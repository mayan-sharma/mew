use std::sync::atomic::Ordering;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, SetWindowsHookExW, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::{CAPTURE_ESCAPE, HELD, HOTKEY_VK, send};
use crate::app::AppEvent;

/// Installs the hook and runs the message loop it needs.
pub fn listen() {
    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), GetModuleHandleW(std::ptr::null()), 0);
        if hook.is_null() {
            log::error!("keyboard hook failed: {}", std::io::Error::last_os_error());
            send(AppEvent::KeyUnavailable);
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
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
