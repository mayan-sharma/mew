//! Wires everything together. Threads: main (tray + message loop), controller (hotkey state machine),
//! worker (speech-to-text, cleanup, paste), models (first-run downloads), plus audio/hotkey/overlay.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};
use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

use crate::asr::Parakeet;
use crate::audio::{Recorder, SAMPLE_RATE};
use crate::config::{self, Hotkey, KeepHistory, Settings};
use crate::learn::{self, Fix};
use crate::llm::Cleaner;
use crate::models::{self, Model};
use crate::overlay::Overlay;
use crate::tray::{self, Tray};
use crate::ui::Ui;
use crate::{api, audio, edits, history, hotkey, paste, system, update, watchdog};

const TAP: Duration = Duration::from_millis(300); // shorter presses are taps, not dictation
const DOUBLE_TAP: Duration = Duration::from_millis(500); // tap, then tap again within this = hands-free
const TAIL: Duration = Duration::from_millis(100); // keep listening briefly so the last word isn't clipped
const MAX_RECORDING: Duration = Duration::from_secs(300);

/// True while the mic is open; the worker won't cover the listening cat with old results.
static RECORDING: AtomicBool = AtomicBool::new(false);
static CLEANUP_ON: AtomicBool = AtomicBool::new(true);

pub enum AppEvent {
    KeyDown,
    KeyUp,
    Escape,
    /// Another key went down while the talk key was held: a shortcut (macOS lets those through), not dictation.
    #[cfg_attr(windows, allow(dead_code))]
    Chord,
    /// The talk key can't be watched (on macOS, until Mew has the Accessibility permission).
    KeyUnavailable,
    SpeechReady,
}

pub enum Command {
    SetHotkey(Hotkey),
    SetMicrophone(Option<String>),
    SetCleanup(bool),
    SetAutostart(bool),
    SetHistory(KeepHistory),
    SetLearn(bool),
    /// Open the Mew window on a page ("overview", "history", "dictionary", "settings").
    Open(&'static str),
    Quit,
}

enum Job {
    Load(Box<Parakeet>),
    Transcribe(Vec<f32>, Instant),
}

pub fn run() -> Result<()> {
    let mut loaded = Settings::load();
    config::ensure_dictionary();
    let first_run = !loaded.welcomed;
    if first_run {
        loaded.welcomed = true;
        if system::is_installed() {
            system::set_autostart(true);
        }
        loaded.save();
    }
    hotkey::HOTKEY_VK.store(loaded.hotkey.vk(), Ordering::Relaxed);
    CLEANUP_ON.store(loaded.cleanup, Ordering::Relaxed);
    if let Err(e) = history::init().and_then(|()| history::prune(loaded.history)) {
        log::error!("history unavailable: {e:#}");
    }
    let hotkey_label = loaded.hotkey;
    let settings = Arc::new(Mutex::new(loaded));

    let recorder = Arc::new(Recorder::new());
    let overlay = Overlay::start({
        let recorder = recorder.clone();
        move || recorder.level()
    });
    overlay.set_hover_tip(&talk_tip());
    let cleaner = Cleaner::new(models::CLEANUP.path().join(models::CLEANUP.files[0].name));

    let (events, inbox) = channel();
    let (jobs, job_queue) = channel();
    let (pushes, pushed) = channel::<Value>();
    spawn("worker", {
        let (overlay, cleaner, settings) = (overlay.clone(), cleaner.clone(), settings.clone());
        move || worker(job_queue, overlay, cleaner, settings, pushes)
    });
    spawn("models", {
        let (events, jobs, overlay) = (events.clone(), jobs.clone(), overlay.clone());
        move || load_models(events, jobs, overlay, hotkey_label, first_run)
    });
    hotkey::start(events.clone());
    spawn("controller", {
        let (overlay, settings) = (overlay.clone(), settings.clone());
        move || {
            Controller {
                settings,
                overlay,
                recorder,
                cleaner,
                jobs,
                speech_ready: false,
                recording: None,
                last_tap: None,
            }
            .run(inbox)
        }
    });

    // Listing audio devices can stall for seconds (Bluetooth, driver hiccups), so it never runs on the tray thread.
    let devices: Arc<Mutex<Vec<String>>> = Arc::default();
    spawn("devices", {
        let devices = devices.clone();
        move || loop {
            let names = audio::input_devices();
            *devices.lock().unwrap() = names;
            std::thread::sleep(Duration::from_secs(5));
        }
    });

    // The tray and the Mew window live on this thread, which runs the event loop they need.
    let mut tray = Tray::new(&settings.lock().unwrap())?;
    let mut ui = Ui::new();
    watchdog::start();
    let beat = &watchdog::MAIN;
    system::run_loop(|| {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            beat.tick(2);
            match tray.handle(&event) {
                Some(Command::Quit) => return false,
                Some(Command::Open(page)) => ui.open(page),
                Some(command) => {
                    apply(command, &settings, &overlay, false);
                    ui.push(&json!({ "type": "settings" }));
                }
                None => {}
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Enter { .. } => {
                    beat.tick(3);
                    let names = devices.lock().unwrap().clone();
                    tray.refresh_mics(&names, settings.lock().unwrap().microphone.as_deref());
                }
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                    if !tray::MENU_ON_CLICK =>
                {
                    ui.open("overview")
                }
                _ => {}
            }
        }
        if watchdog::take_open_request() {
            ui.open("overview");
        }
        while let Ok(event) = pushed.try_recv() {
            ui.push(&event);
        }
        if ui.is_open() {
            beat.tick(4);
            let mut ctx = api::Ctx { settings: &settings, overlay: &overlay, tray: &mut tray, devices: &devices };
            ui.pump(&mut |method, params| api::handle(method, params, &mut ctx));
        }
        beat.tick(0);
        true
    });
    Ok(())
}

/// Applies a settings change from the tray, the window, or the controller. Thread-safe.
/// `quiet` skips the cat's spoken confirmation (the window shows its own).
pub fn apply(command: Command, settings: &Mutex<Settings>, overlay: &Overlay, quiet: bool) {
    let mut s = settings.lock().unwrap();
    let say = |text: &str| {
        if !quiet {
            overlay.say(text);
        }
    };
    match command {
        Command::SetHotkey(hotkey) => {
            s.hotkey = hotkey;
            hotkey::HOTKEY_VK.store(hotkey.vk(), Ordering::Relaxed);
            overlay.set_hover_tip(&talk_tip());
            say(&format!("Got it! Hold {} to talk.", hotkey.label()));
        }
        Command::SetMicrophone(name) => {
            let default = if cfg!(target_os = "macos") { "the system default mic" } else { "the Windows default mic" };
            say(&format!("I'll listen with {}.", name.as_deref().unwrap_or(default)));
            s.microphone = name;
        }
        Command::SetCleanup(on) => {
            s.cleanup = on;
            CLEANUP_ON.store(on, Ordering::Relaxed);
            say(if on { "I'll tidy up your words." } else { "I'll type exactly what I hear." });
        }
        Command::SetAutostart(on) => system::set_autostart(on),
        Command::SetLearn(on) => s.learn = on,
        Command::SetHistory(keep) => {
            s.history = keep;
            if let Err(e) = history::prune(keep) {
                log::warn!("pruning history failed: {e:#}");
            }
        }
        Command::Open(_) | Command::Quit => return,
    }
    s.save();
}

struct Recording {
    started: Instant,
    locked: bool,
}

struct Controller {
    settings: Arc<Mutex<Settings>>,
    overlay: Overlay,
    recorder: Arc<Recorder>,
    cleaner: Cleaner,
    jobs: Sender<Job>,
    speech_ready: bool,
    recording: Option<Recording>,
    last_tap: Option<Instant>,
}

impl Controller {
    fn run(mut self, inbox: Receiver<AppEvent>) {
        let mut next_update_check = Instant::now() + Duration::from_secs(60);
        let mut next_prune = Instant::now() + Duration::from_secs(3600);
        loop {
            match inbox.recv_timeout(Duration::from_millis(250)) {
                Ok(AppEvent::KeyDown) => self.key_down(),
                Ok(AppEvent::KeyUp) => self.key_up(),
                Ok(AppEvent::Escape) => {
                    if self.recording.is_some() {
                        self.cancel();
                        self.overlay.huh("Okay, never mind.");
                    }
                }
                Ok(AppEvent::Chord) => {
                    if self.recording.as_ref().is_some_and(|r| !r.locked) {
                        self.cancel();
                    }
                }
                Ok(AppEvent::KeyUnavailable) => self.overlay.say(if cfg!(target_os = "macos") {
                    "To hear the talk key, I need your OK: turn on Mew in System Settings › Privacy & Security › Accessibility."
                } else {
                    "I can't hear the talk key. Try restarting Mew."
                }),
                Ok(AppEvent::SpeechReady) => self.speech_ready = true,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if self.recording.as_ref().is_some_and(|r| r.started.elapsed() > MAX_RECORDING) {
                self.finish();
            }
            self.cleaner.stop_if_idle();
            if Instant::now() > next_update_check {
                update::check(&self.overlay, false);
                next_update_check = Instant::now() + Duration::from_secs(12 * 3600);
            }
            if Instant::now() > next_prune {
                let keep = self.settings.lock().unwrap().history;
                let _ = history::prune(keep);
                next_prune = Instant::now() + Duration::from_secs(3600);
            }
        }
    }

    fn key_down(&mut self) {
        match &self.recording {
            Some(r) if r.locked => return self.finish(), // second press ends hands-free
            Some(_) => return,
            None => {}
        }
        if !self.speech_ready {
            self.overlay.huh("Almost ready! I'm still getting my ears on.");
            return;
        }
        let microphone = self.settings.lock().unwrap().microphone.clone();
        if let Err(e) = self.recorder.start(microphone) {
            log::warn!("microphone failed: {e:#}");
            self.overlay.huh("I can't hear you. Check that a mic is connected and allowed in Settings.");
            return;
        }
        RECORDING.store(true, Ordering::Relaxed);
        hotkey::CAPTURE_ESCAPE.store(true, Ordering::Relaxed);
        self.overlay.listen(false);
        if CLEANUP_ON.load(Ordering::Relaxed) {
            self.cleaner.warm(); // loads while you talk
        }
        self.recording = Some(Recording { started: Instant::now(), locked: false });
    }

    fn key_up(&mut self) {
        let Some(recording) = &mut self.recording else { return };
        if recording.locked {
            return;
        }
        if recording.started.elapsed() >= TAP {
            return self.finish();
        }
        // A tap. Two taps in a row switch to hands-free; a lone tap is ignored.
        if self.last_tap.is_some_and(|t| recording.started.duration_since(t) < DOUBLE_TAP) {
            recording.locked = true;
            self.last_tap = None;
            self.overlay.listen(true);
        } else {
            self.cancel();
            self.last_tap = Some(Instant::now());
        }
    }

    fn finish(&mut self) {
        std::thread::sleep(TAIL);
        let audio = self.recorder.stop();
        let released = Instant::now();
        self.recording = None;
        RECORDING.store(false, Ordering::Relaxed);
        hotkey::CAPTURE_ESCAPE.store(false, Ordering::Relaxed);
        self.overlay.think();
        let _ = self.jobs.send(Job::Transcribe(audio, released));
    }

    fn cancel(&mut self) {
        self.recorder.stop();
        self.recording = None;
        RECORDING.store(false, Ordering::Relaxed);
        hotkey::CAPTURE_ESCAPE.store(false, Ordering::Relaxed);
        self.overlay.idle();
    }
}

fn worker(
    jobs: Receiver<Job>,
    overlay: Overlay,
    cleaner: Cleaner,
    settings: Arc<Mutex<Settings>>,
    pushes: Sender<Value>,
) {
    let mut asr: Option<Box<Parakeet>> = None;
    let mut last_paste: Option<(isize, Instant, String)> = None; // (window, when, text)
    for job in jobs {
        let (audio, released) = match job {
            Job::Load(model) => {
                asr = Some(model);
                continue;
            }
            Job::Transcribe(audio, released) => (audio, released),
        };
        let Some(asr) = asr.as_mut() else { continue };
        let show = |f: &dyn Fn()| {
            if !RECORDING.load(Ordering::Relaxed) {
                f();
            }
        };
        let t0 = Instant::now();
        let raw = match asr.transcribe(&audio) {
            Ok(text) => text,
            Err(e) => {
                log::error!("transcription failed: {e:#}");
                show(&|| overlay.huh("Oops, something went wrong."));
                continue;
            }
        };
        let t1 = Instant::now();
        if raw.is_empty() {
            show(&|| overlay.huh("Hmm, I didn't catch that."));
            continue;
        }
        let text = if CLEANUP_ON.load(Ordering::Relaxed) { cleaner.clean(&raw) } else { raw.clone() };
        let text = learn::apply(&text, &history::fixes().unwrap_or_default()); // spellings learned from your edits
        let t2 = Instant::now();

        // Back-to-back dictations into the same window get a joining space.
        let window = system::foreground_window();
        let joins = last_paste.as_ref().is_some_and(|(w, at, prev)| {
            *w == window && at.elapsed() < Duration::from_secs(120) && prev.ends_with(['.', '!', '?', ',', ';', ':'])
        });
        let typed = if joins { format!(" {text}") } else { text.clone() };
        if let Err(e) = paste::paste(&typed) {
            log::error!("paste failed: {e:#}");
            show(&|| overlay.huh("I couldn't type here. Try clicking into a text box."));
            continue;
        }
        last_paste = Some((window, Instant::now(), text.clone()));
        show(&|| overlay.say(&text));

        if settings.lock().unwrap().learn {
            let (overlay, pushes) = (overlay.clone(), pushes.clone());
            edits::watch(text.clone(), move |fixes| remember(&fixes, &overlay, &pushes));
        }

        let keep_text = settings.lock().unwrap().history != KeepHistory::Off;
        let saved = history::add(&history::NewEntry {
            app: &system::app_name(window),
            text: &text,
            raw: &raw,
            audio_ms: (audio.len() as u64 * 1000 / SAMPLE_RATE as u64) as i64,
            asr_ms: (t1 - t0).as_millis() as i64,
            cleanup_ms: (t2 - t1).as_millis() as i64,
            latency_ms: ((t2 - released) + TAIL).as_millis() as i64,
            keep_text,
        });
        match saved {
            Ok(entry) => {
                let _ = pushes.send(json!({ "type": "dictation", "entry": entry }));
                system::wake(); // the window's thread
            }
            Err(e) => log::warn!("saving to history failed: {e:#}"),
        }
        log::info!(
            "audio {:.1}s | asr {}ms | cleanup {}ms{} | release->paste {}ms | {} words",
            audio.len() as f32 / SAMPLE_RATE as f32,
            (t1 - t0).as_millis(),
            (t2 - t1).as_millis(),
            if text == raw { " (as heard)" } else { "" },
            (t2 - released).as_millis() + TAIL.as_millis(),
            text.split_whitespace().count()
        );
    }
}

/// Saves spelling fixes learned from the user's edits and lets them know.
fn remember(fixes: &[Fix], overlay: &Overlay, pushes: &Sender<Value>) {
    let mut terms = config::dictionary();
    for fix in fixes {
        if let Err(e) = history::add_fix(fix) {
            log::warn!("saving a learned fix failed: {e:#}");
            return;
        }
        if !terms.iter().any(|t| t.eq_ignore_ascii_case(&fix.fixed)) {
            terms.push(fix.fixed.clone());
        }
    }
    config::set_dictionary(&terms);
    log::info!("learned {} spelling fix(es) from an edit", fixes.len());
    if !RECORDING.load(Ordering::Relaxed) {
        match fixes {
            [one] => overlay.say(&format!("Noted! I'll write “{}” from now on.", one.fixed)),
            _ => overlay.say("Noted! I'll remember those spellings."),
        }
    }
    let _ = pushes.send(json!({ "type": "dictionary" }));
    system::wake();
}

fn load_models(events: Sender<AppEvent>, jobs: Sender<Job>, overlay: Overlay, hotkey: Hotkey, first_run: bool) {
    let welcome = if first_run { "Hi, I'm Mew! " } else { "" };
    download(&models::SPEECH, &overlay, &format!("{welcome}Getting my ears ready…"), true);
    loop {
        match Parakeet::load(&models::SPEECH.path()) {
            Ok(model) => {
                let _ = jobs.send(Job::Load(Box::new(model)));
                let _ = events.send(AppEvent::SpeechReady);
                break;
            }
            Err(e) => {
                log::error!("loading speech model failed: {e:#}");
                overlay.huh("My ears aren't working. I'll try again in a minute.");
                std::thread::sleep(Duration::from_secs(60));
            }
        }
    }
    overlay.idle();
    if first_run {
        overlay.say(&format!("All set! Hold {} anywhere and talk to me.", hotkey.label()));
    }
    download(&models::CLEANUP, &overlay, "Learning to tidy up text…", false);
}

/// Downloads until it succeeds. `loud` shows progress in a bubble; otherwise only on hover.
fn download(model: &Model, overlay: &Overlay, label: &str, loud: bool) {
    if model.is_ready() {
        return;
    }
    let total = model.total_size() as f32;
    loop {
        let report = |done: u64| {
            let fraction = done as f32 / total;
            let text = format!("{label} {:.0}%", fraction * 100.0);
            if loud {
                overlay.progress(&text, fraction);
            }
            overlay.set_hover_tip(&text);
        };
        match model.download(&report) {
            Ok(()) => break,
            Err(e) => {
                log::error!("download failed: {e:#}");
                if loud {
                    overlay.progress("I can't reach the internet to get ready. Retrying in a minute…", 0.0);
                }
                std::thread::sleep(Duration::from_secs(60));
            }
        }
    }
    overlay.set_hover_tip(&talk_tip());
}

fn talk_tip() -> String {
    let vk = hotkey::HOTKEY_VK.load(Ordering::Relaxed);
    let key = Hotkey::ALL.iter().find(|h| h.vk() == vk).map_or("your talk key", |h| h.label());
    format!("Hold {key} and talk to me")
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name(name.into()).spawn(f).expect("spawn thread");
}
