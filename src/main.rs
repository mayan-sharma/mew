#![windows_subsystem = "windows"]

mod api;
mod app;
mod asr;
mod audio;
mod cat;
mod config;
mod edits;
mod history;
mod hotkey;
mod learn;
mod llm;
mod logger;
mod models;
mod overlay;
mod paste;
mod system;
mod tray;
mod ui;
mod update;
mod watchdog;

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
#[cfg(windows)]
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

fn main() {
    let mut velopack = velopack::VelopackApp::build();
    // macOS apps have no uninstaller; Mew.app is dragged to the Trash.
    #[cfg(windows)]
    {
        velopack = velopack.on_before_uninstall_fast_callback(|_| {
            system::set_autostart(false);
            let _ = std::fs::remove_dir_all(config::data_dir()); // models, settings, logs
        });
    }
    velopack.run();

    // Older macOS hands apps opened from Finder a -psn_ argument.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    if !args.is_empty() {
        #[cfg(windows)]
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS)
        };
        logger::init(true);
        if let Err(e) = cli(&args) {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
        return;
    }

    if !system::first_instance() {
        return;
    }
    system::init();
    logger::init(false);
    log::info!("Mew {} starting", env!("CARGO_PKG_VERSION"));
    if let Err(e) = app::run() {
        log::error!("fatal: {e:#}");
    }
}

/// Developer and support commands; the app itself takes no arguments.
fn cli(args: &[String]) -> Result<()> {
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--selftest", wav] => selftest(Path::new(wav)),
        ["--seed-demo"] => {
            // Only into a scratch data folder, never the real history.
            anyhow::ensure!(std::env::var_os("MEW_DATA_DIR").is_some(), "set MEW_DATA_DIR to a scratch folder first");
            history::init()?;
            history::seed_demo()
        }
        ["--download"] => {
            for model in [&models::SPEECH, &models::CLEANUP] {
                let total = model.total_size();
                model.download(&|done| eprint!("\r{}: {:.0}%   ", model.dir, done as f64 / total as f64 * 100.0))?;
                eprintln!("\r{}: ready      ", model.dir);
            }
            Ok(())
        }
        ["--render", dir] => render_states(Path::new(dir)),
        ["--write-icon", path] => std::fs::write(path, cat::icon_ico()).context("writing icon"),
        ["--write-icns", path] => std::fs::write(path, cat::icon_icns()).context("writing icon"),
        _ => bail!(
            "usage: mew [--download | --selftest <16k-mono.wav> | --render <dir> | --write-icon <file.ico> | \
             --write-icns <file.icns>]"
        ),
    }
}

/// Speech-to-text and cleanup on a WAV file with the downloaded models; nothing is typed.
fn selftest(wav: &Path) -> Result<()> {
    let audio = read_wav(wav)?;
    let t = Instant::now();
    let mut asr = asr::Parakeet::load(&models::SPEECH.path()).context("speech model (run Mew once to download it)")?;
    println!("speech model loaded in {:?}", t.elapsed());
    let cleaner = llm::Cleaner::new(models::CLEANUP.path().join(models::CLEANUP.files[0].name));
    let t = Instant::now();
    cleaner.warm();
    while !cleaner.ready() && t.elapsed() < Duration::from_secs(120) {
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("cleanup model {} in {:?}", if cleaner.ready() { "ready" } else { "NOT ready" }, t.elapsed());
    std::thread::sleep(Duration::from_millis(500)); // let the prompt cache prime
    let t = Instant::now();
    let raw = asr.transcribe(&audio)?;
    let asr_time = t.elapsed();
    let t = Instant::now();
    let clean = cleaner.clean(&raw);
    println!("audio {:.1}s | asr {asr_time:?} | cleanup {:?}", audio.len() as f32 / 16_000.0, t.elapsed());
    println!("heard: {raw}\ntyped: {clean}");
    Ok(())
}

/// Writes one PNG per overlay state, for checking the drawing.
fn render_states(dir: &Path) -> Result<()> {
    use cat::{Mode, Scene};
    std::fs::create_dir_all(dir)?;
    let renderer = cat::Renderer::new();
    let levels: Vec<f32> = (0..30)
        .map(|i| ((i as f32 * 0.7).sin().abs() * (0.4 + 0.6 * (i as f32 * 0.23).sin().abs())).min(1.0))
        .collect();
    let scene = |mode: Mode, text: &str| Scene {
        mode,
        t: 0.5,
        pill_w: mode.pill_width(),
        levels: levels.clone(),
        locked: false,
        text: text.into(),
        progress: None,
    };
    let states = [
        ("idle", scene(Mode::Idle, "")),
        ("listening", scene(Mode::Listening, "")),
        ("hands-free", Scene { locked: true, ..scene(Mode::Listening, "") }),
        ("thinking", scene(Mode::Thinking, "")),
        ("said", scene(Mode::Said, "I was thinking we should meet on Wednesday at 4. And can you bring the slides?")),
        ("huh", scene(Mode::Huh, "Hmm, I didn't catch that.")),
        ("download", Scene { progress: Some(0.42), ..scene(Mode::Idle, "Hi, I'm Mew! Getting my ears ready… 42%") }),
    ];
    for (name, scene) in states {
        renderer.render(&scene, 2.0).save_png(dir.join(format!("{name}.png")))?;
    }
    std::fs::write(dir.join("icon.png"), cat::icon_png(64))?;
    Ok(())
}

/// 16 kHz mono PCM16 WAV.
fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    let at = bytes.windows(4).position(|w| w == b"data").context("no data chunk")?;
    let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
    let data = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
    Ok(data.as_chunks::<2>().0.iter().map(|&s| i16::from_le_bytes(s) as f32 / 32768.0).collect())
}
