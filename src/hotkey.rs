//! Global hold-to-talk key: a low-level keyboard hook on Windows, an event tap on macOS. The hotkey never reaches
//! other apps (on macOS, unless it's part of a shortcut); Escape is captured only while Mew is recording.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::mpsc::Sender;

use crate::app::AppEvent;

#[cfg_attr(windows, path = "hotkey/windows.rs")]
#[cfg_attr(target_os = "macos", path = "hotkey/macos.rs")]
mod platform;

/// The talk key's platform key code, from `Hotkey::vk`.
pub static HOTKEY_VK: AtomicU32 = AtomicU32::new(0);
/// Set while recording so Escape cancels instead of reaching the focused app.
pub static CAPTURE_ESCAPE: AtomicBool = AtomicBool::new(false);
static HELD: AtomicBool = AtomicBool::new(false);
static EVENTS: OnceLock<Sender<AppEvent>> = OnceLock::new();

pub fn start(events: Sender<AppEvent>) {
    let _ = EVENTS.set(events);
    std::thread::Builder::new().name("hotkey".into()).spawn(platform::listen).expect("spawn hotkey thread");
}

fn send(event: AppEvent) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.send(event);
    }
}
