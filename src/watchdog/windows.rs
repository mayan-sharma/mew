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

/// Creates the window that answers pings (on the tray thread) and returns how to ping it.
pub fn pinger() -> impl Fn() + Send + 'static {
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
    move || {
        unsafe { PostMessageW(hwnd as HWND, WM_PING, 0, 0) };
    }
}

unsafe extern "system" fn ping_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_PING {
        super::MAIN.alive();
        return 0;
    }
    if msg == WM_OPEN {
        super::request_open();
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// From a second launch: ask the running Mew to open its window. Returns false if none was found.
pub fn signal_running_instance() -> bool {
    let class = wide(CLASS);
    unsafe {
        let hwnd = FindWindowExW(HWND_MESSAGE, std::ptr::null_mut(), class.as_ptr(), std::ptr::null());
        !hwnd.is_null() && PostMessageW(hwnd, WM_OPEN, 0, 0) != 0
    }
}
