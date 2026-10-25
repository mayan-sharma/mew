//! Global hold-to-talk key via a low-level keyboard hook. The hotkey never reaches other apps;
//! Escape is captured only while Mew is recording.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, SetWindowsHookExW, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::app::AppEvent;

pub static HOTKEY_VK: AtomicU32 = AtomicU32::new(0xA5);
/// Set while recording so Escape cancels instead of reaching the focused app.
pub static CAPTURE_ESCAPE: AtomicBool = AtomicBool::new(false);
static HELD: AtomicBool = AtomicBool::new(false);
static EVENTS: OnceLock<Sender<AppEvent>> = OnceLock::new();

pub fn start(events: Sender<AppEvent>) {
    let _ = EVENTS.set(events);
    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(|| unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), GetModuleHandleW(std::ptr::null()), 0);
            if hook.is_null() {
                log::error!("keyboard hook failed: {}", std::io::Error::last_os_error());
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        })
        .expect("spawn hotkey thread");
}

fn send(event: AppEvent) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.send(event);
    }
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if kb.flags & LLKHF_INJECTED == 0 {
            let msg = wparam as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
            if kb.vkCode == HOTKEY_VK.load(Ordering::Relaxed) {
                if down && !HELD.swap(true, Ordering::Relaxed) {
                    send(AppEvent::KeyDown);
                } else if up && HELD.swap(false, Ordering::Relaxed) {
                    send(AppEvent::KeyUp);
                }
                return 1; // swallow: no Alt-menu flashes, no Caps Lock toggles
            }
            if kb.vkCode == VK_ESCAPE as u32 && down && CAPTURE_ESCAPE.load(Ordering::Relaxed) {
                send(AppEvent::Escape);
                return 1;
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}
