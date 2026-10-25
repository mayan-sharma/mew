//! Logs which UI thread stopped responding, and what it was doing, so a "not responding" leaves evidence.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterClassW, WM_APP, WNDCLASSW,
};

use crate::overlay::wide;

/// Posted to a message-only window on the tray thread every second. Window messages keep flowing
/// inside modal loops (an open tray menu), so only a real hang stops the replies.
const WM_PING: u32 = WM_APP + 1;
/// Sent by a second launch of mew.exe (e.g. from the Start menu): open the Mew window.
const WM_OPEN: u32 = WM_APP + 2;
const CLASS: &str = "MewWatchdog";
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
    let hwnd = unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(ping_proc),
            hInstance: hinst,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        )
    } as isize;
    std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            let mut stalled = [false; 2];
            loop {
                std::thread::sleep(Duration::from_secs(1));
                unsafe { PostMessageW(hwnd as HWND, WM_PING, 0, 0) };
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

unsafe extern "system" fn ping_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_PING {
        MAIN.alive();
        return 0;
    }
    if msg == WM_OPEN {
        OPEN_REQUESTED.store(true, Ordering::Relaxed);
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// True once after another mew.exe asked this one to open its window.
pub fn take_open_request() -> bool {
    OPEN_REQUESTED.swap(false, Ordering::Relaxed)
}

/// From a second launch: ask the running Mew to open its window. Returns false if none was found.
pub fn signal_running_instance() -> bool {
    let class = wide(CLASS);
    unsafe {
        let hwnd = FindWindowExW(HWND_MESSAGE, std::ptr::null_mut(), class.as_ptr(), std::ptr::null());
        !hwnd.is_null() && PostMessageW(hwnd, WM_OPEN, 0, 0) != 0
    }
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
