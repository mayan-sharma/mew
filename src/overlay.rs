//! Mew's on-screen presence: a click-through window at the bottom center of the screen you're working on.
//! Never takes focus; on Windows it also hides while a fullscreen app is in front.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::cat::{Mode, Renderer, Scene};

#[cfg_attr(windows, path = "overlay/windows.rs")]
#[cfg_attr(target_os = "macos", path = "overlay/macos.rs")]
mod platform;

#[cfg(windows)]
pub use platform::wide;

const FPS: u64 = 30;

struct State {
    mode: Mode,
    text: String,
    progress: Option<f32>,
    locked: bool,
    since: Instant,
    until: Option<Instant>,
    hover_tip: String,
    retarget: bool,
}

#[derive(Clone)]
pub struct Overlay {
    state: Arc<Mutex<State>>,
}

impl Overlay {
    pub fn start(level: impl Fn() -> f32 + Send + 'static) -> Self {
        let state = Arc::new(Mutex::new(State {
            mode: Mode::Idle,
            text: String::new(),
            progress: None,
            locked: false,
            since: Instant::now(),
            until: None,
            hover_tip: String::new(),
            retarget: true,
        }));
        platform::start(state.clone(), level);
        Self { state }
    }

    fn set(&self, mode: Mode, text: &str, hold: Option<Duration>) {
        let mut s = self.state.lock().unwrap();
        s.mode = mode;
        s.text = text.to_string();
        s.progress = None;
        s.since = Instant::now();
        s.until = hold.map(|d| s.since + d);
        if mode == Mode::Listening {
            s.retarget = true; // follow the monitor you're working on
        } else {
            s.locked = false;
        }
    }

    pub fn idle(&self) {
        self.set(Mode::Idle, "", None);
    }

    pub fn listen(&self, locked: bool) {
        self.set(Mode::Listening, "", None);
        self.state.lock().unwrap().locked = locked;
    }

    pub fn think(&self) {
        self.set(Mode::Thinking, "", None);
    }

    /// Happy face with a speech bubble, then back to sleep.
    pub fn say(&self, text: &str) {
        let hold = Duration::from_secs_f32((2.0 + 0.04 * text.chars().count() as f32).min(6.0));
        self.set(Mode::Said, text, Some(hold));
    }

    /// Surprised face with a short message.
    pub fn huh(&self, text: &str) {
        self.set(Mode::Huh, text, Some(Duration::from_secs(3)));
    }

    /// Sleepy cat with a bubble and progress bar that stays until replaced.
    pub fn progress(&self, text: &str, fraction: f32) {
        let mut s = self.state.lock().unwrap();
        if s.mode != Mode::Idle || s.until.is_some() {
            return; // don't interrupt dictation feedback
        }
        s.text = text.to_string();
        s.progress = Some(fraction);
    }

    pub fn set_hover_tip(&self, tip: &str) {
        self.state.lock().unwrap().hover_tip = tip.to_string();
    }
}

/// Turns the shared state into frames for the platform window.
struct Animator<L> {
    state: Arc<Mutex<State>>,
    level: L,
    renderer: Renderer,
    win: platform::Window,
    levels: VecDeque<f32>,
    pill_w: f32,
    last: Option<(Mode, String, Option<u32>, bool, u32)>,
}

impl<L: Fn() -> f32> Animator<L> {
    fn new(state: Arc<Mutex<State>>, level: L, win: platform::Window) -> Self {
        Self {
            state,
            level,
            renderer: Renderer::new(),
            win, // created hidden; the first frame shows it
            levels: VecDeque::with_capacity(40),
            pill_w: Mode::Idle.pill_width(),
            last: None,
        }
    }

    /// Draws a frame if anything changed, and returns how long to wait before the next one.
    fn frame(&mut self) -> Duration {
        let beat = &crate::watchdog::OVERLAY;
        let now = Instant::now();
        let win = &mut self.win;
        let scene = {
            let mut s = self.state.lock().unwrap();
            if s.until.is_some_and(|u| now > u) {
                *s = State {
                    mode: Mode::Idle,
                    text: String::new(),
                    progress: None,
                    locked: false,
                    since: now,
                    until: None,
                    hover_tip: std::mem::take(&mut s.hover_tip),
                    retarget: false,
                };
            }
            if std::mem::take(&mut s.retarget) || win.size.0 == 0 {
                win.place();
                self.last = None;
            }
            let mut text = s.text.clone();
            if s.mode == Mode::Idle && text.is_empty() && win.hovered(self.pill_w) {
                text = s.hover_tip.clone();
            }
            Scene {
                mode: s.mode,
                t: (now - s.since).as_secs_f32(),
                pill_w: self.pill_w,
                levels: Vec::new(),
                locked: s.locked,
                text,
                progress: s.progress,
            }
        };

        // Get out of the way of games, videos and presentations while idle.
        let hide = scene.mode == Mode::Idle && scene.text.is_empty() && platform::fullscreen_app();
        if hide == win.visible {
            beat.tick(2);
            win.show(!hide);
        }

        if scene.mode == Mode::Listening {
            if self.levels.len() == self.levels.capacity() {
                self.levels.pop_front();
            }
            self.levels.push_back(((self.level)() * 18.0).powf(0.7).min(1.0));
        } else {
            self.levels.clear();
        }
        let target = scene.mode.pill_width();
        let pill_w = self.pill_w;
        self.pill_w = if (target - pill_w).abs() < 0.5 { target } else { pill_w + (target - pill_w) * 0.35 };
        let animating = matches!(scene.mode, Mode::Listening | Mode::Thinking) || self.pill_w != target;
        let key = (
            scene.mode,
            scene.text.clone(),
            scene.progress.map(|p| (p * 1000.0) as u32),
            scene.locked,
            self.pill_w.to_bits(),
        );
        if win.visible && (animating || self.last.as_ref() != Some(&key)) {
            let scene = Scene { pill_w: self.pill_w, levels: self.levels.iter().copied().collect(), ..scene };
            beat.tick(1);
            win.blit(&self.renderer.render(&scene, win.scale));
            self.last = Some(key);
        }
        Duration::from_millis(if animating { 1000 / FPS } else { 100 })
    }
}
