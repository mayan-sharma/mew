//! Logs which UI thread stopped responding, and what it was doing, so a "not responding" leaves evidence.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[cfg_attr(windows, path = "watchdog/windows.rs")]
#[cfg_attr(target_os = "macos", path = "watchdog/macos.rs")]
mod platform;

#[cfg(windows)]
pub use platform::signal_running_instance;

static OPEN_REQUESTED: AtomicBool = AtomicBool::new(false);
const STALL: Duration = Duration::from_secs(3);

pub struct Beat {
    name: &'static str,
    phases: &'static [&'static str],
    at_ms: AtomicU64,
    phase: AtomicU8,
}

impl Beat {
    pub const fn new(name: &'static str, phases: &'static [&'static str]) -> Self {
        Self { name, phases, at_ms: AtomicU64::new(0), phase: AtomicU8::new(0) }
    }

    /// Records that the thread is alive and is now in `phase` (an index into `phases`).
    pub fn tick(&self, phase: u8) {
        self.alive();
        self.phase.store(phase, Ordering::Relaxed);
    }

    fn alive(&self) {
        self.at_ms.store(now_ms(), Ordering::Relaxed);
    }
}

pub static MAIN: Beat = Beat::new("tray", &["idle", "window message", "menu click", "mic list", "Mew window"]);
pub static OVERLAY: Beat = Beat::new("overlay", &["idle", "drawing", "showing window"]);

/// Call on the tray thread, after its window exists.
pub fn start() {
    MAIN.tick(0);
    OVERLAY.tick(0);
    let ping = platform::pinger();
    std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            let mut stalled = [false; 2];
            loop {
                std::thread::sleep(Duration::from_secs(1));
                ping();
                for (i, beat) in [&MAIN, &OVERLAY].into_iter().enumerate() {
                    let quiet = now_ms().saturating_sub(beat.at_ms.load(Ordering::Relaxed));
                    let phase = beat.phases.get(beat.phase.load(Ordering::Relaxed) as usize).unwrap_or(&"?");
                    if quiet > STALL.as_millis() as u64 && !stalled[i] {
                        log::error!(
                            "{} thread unresponsive for {:.1}s during {phase}",
                            beat.name,
                            quiet as f32 / 1000.0
                        );
                        stalled[i] = true;
                    } else if quiet < STALL.as_millis() as u64 && stalled[i] {
                        log::warn!("{} thread recovered", beat.name);
                        stalled[i] = false;
                    }
                }
            }
        })
        .expect("spawn watchdog");
}

/// Another launch of Mew asked this one to open its window.
pub fn request_open() {
    OPEN_REQUESTED.store(true, Ordering::Relaxed);
}

/// True once after another launch of Mew asked this one to open its window.
pub fn take_open_request() -> bool {
    OPEN_REQUESTED.swap(false, Ordering::Relaxed)
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
