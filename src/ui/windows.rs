use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicBool, Ordering};

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_TERMINATE, TerminateProcess};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, IsIconic, LoadIconW, MINMAXINFO, RegisterClassW, SPI_GETWORKAREA,
    SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow, SystemParametersInfoW, WM_CLOSE, WM_GETMINMAXINFO,
    WM_SETTINGCHANGE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use wry::{WebView, WebViewBuilder};

use super::CLOSE_REQUESTED;
use crate::overlay::wide;

const SIZE: (i32, i32) = (1040, 720);
const MIN_SIZE: (i32, i32) = (760, 540);

static THEME_CHANGED: AtomicBool = AtomicBool::new(false);

pub struct Window {
    hwnd: HWND,
    pub webview: WebView,
}

struct Handle(HWND);

impl HasWindowHandle for Handle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = Win32WindowHandle::new(NonZeroIsize::new(self.0 as isize).ok_or(HandleError::Unavailable)?);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl Window {
    pub fn create(builder: WebViewBuilder, dark: bool) -> anyhow::Result<Self> {
        let hwnd = unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let class = wide("MewWindow");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                hIcon: LoadIconW(hinst, 1 as _), // icon resource 1 (build.rs)
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            let scale = GetDpiForSystem() as f32 / 96.0;
            let (w, h) = ((SIZE.0 as f32 * scale) as i32, (SIZE.1 as f32 * scale) as i32);
            let mut work = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
            let (w, h) = (w.min(work.right - work.left), h.min(work.bottom - work.top));
            let (x, y) = (work.left + (work.right - work.left - w) / 2, work.top + (work.bottom - work.top - h) / 2);
            let title = wide("Mew");
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                x,
                y,
                w,
                h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            )
        };
        anyhow::ensure!(!hwnd.is_null(), "CreateWindow failed");
        style_title_bar(hwnd, dark);
        let webview = builder.build(&Handle(hwnd))?;
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
        Ok(Self { hwnd, webview })
    }

    pub fn focus(&self) {
        unsafe {
            if IsIconic(self.hwnd) != 0 {
                ShowWindow(self.hwnd, SW_RESTORE);
            }
            SetForegroundWindow(self.hwnd);
        }
    }

    pub fn follow_theme(&self) {
        if THEME_CHANGED.swap(false, Ordering::Relaxed) {
            style_title_bar(self.hwnd, dark_mode());
        }
    }

    pub fn close(self) {
        let Self { hwnd, webview } = self;
        drop(webview); // WebView first, then its parent window
        unsafe { DestroyWindow(hwnd) };
        end_webview_processes();
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            CLOSE_REQUESTED.store(true, Ordering::Relaxed); // the main loop tears down the WebView
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
            info.ptMinTrackSize.x = (MIN_SIZE.0 as f32 * scale) as i32;
            info.ptMinTrackSize.y = (MIN_SIZE.1 as f32 * scale) as i32;
            0
        }
        WM_SETTINGCHANGE => {
            THEME_CHANGED.store(true, Ordering::Relaxed);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// WebView2 keeps its browser processes (~360 MB) alive after the WebView is closed, because wry retains
/// references to the environment. The window is closed, so end the browser processes Mew started; their
/// renderer, GPU and utility children exit with them. They only hold a cache for Mew's own data folder.
fn end_webview_processes() {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return;
        }
        let me = GetCurrentProcessId();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ended = 0;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if entry.th32ParentProcessID == me && name.eq_ignore_ascii_case("msedgewebview2.exe") {
                let process = OpenProcess(PROCESS_TERMINATE, 0, entry.th32ProcessID);
                if !process.is_null() {
                    ended += (TerminateProcess(process, 0) != 0) as u32;
                    CloseHandle(process);
                }
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        log::info!("Mew window closed; ended {ended} WebView process(es)");
    }
}

/// Dark title bar in dark mode, and a caption that matches the page background on Windows 11.
fn style_title_bar(hwnd: HWND, dark: bool) {
    let on: i32 = dark.into();
    let caption: u32 = if dark { 0x0017191c } else { 0x00f4f8fb }; // COLORREF is 0x00BBGGRR
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, (&on as *const i32).cast(), 4);
        DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR as u32, (&caption as *const u32).cast(), 4);
    }
}

/// Windows "app mode" setting (Settings > Personalization > Colors).
pub fn dark_mode() -> bool {
    let (key, value) =
        (wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), wide("AppsUseLightTheme"));
    let mut light = 1u32;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut light as *mut u32).cast(),
            &mut size,
        )
    };
    light == 0
}
