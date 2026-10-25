//! The Mew window: history, stats, dictionary and settings, as a page inside a WebView2 window.
//! The page is built from `ui/` and embedded in mew.exe; it talks to Rust over `window.ipc`.
//! Closing the window destroys the WebView, so it costs nothing while closed.

use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use serde_json::{Value, json};
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
use wry::http::Request;
use wry::{WebContext, WebView, WebViewBuilder};

use crate::config;
use crate::overlay::wide;

const PAGE: &str = include_str!("../ui/dist/index.html");
const SIZE: (i32, i32) = (1040, 720);
const MIN_SIZE: (i32, i32) = (760, 540);

static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
static THEME_CHANGED: AtomicBool = AtomicBool::new(false);

pub struct Ui {
    window: Option<Window>,
    context: WebContext,
    requests: Receiver<String>,
    request_tx: Sender<String>,
}

struct Window {
    hwnd: HWND,
    webview: WebView,
}

struct Handle(HWND);

impl HasWindowHandle for Handle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = Win32WindowHandle::new(NonZeroIsize::new(self.0 as isize).ok_or(HandleError::Unavailable)?);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl Ui {
    pub fn new() -> Self {
        let (request_tx, requests) = channel();
        let context = WebContext::new(Some(config::data_dir().join("webview")));
        Self { window: None, context, requests, request_tx }
    }

    pub fn is_open(&self) -> bool {
        self.window.is_some()
    }

    /// Opens the window, or brings it to the front if it's already open. `page` picks the starting tab.
    pub fn open(&mut self, page: &str) {
        if let Some(w) = &self.window {
            unsafe {
                if IsIconic(w.hwnd) != 0 {
                    ShowWindow(w.hwnd, SW_RESTORE);
                }
                SetForegroundWindow(w.hwnd);
            }
            self.push(&json!({ "type": "navigate", "page": page }));
            return;
        }
        match self.create(page) {
            Ok(window) => self.window = Some(window),
            Err(e) => {
                log::error!("opening the Mew window failed: {e:#}");
                crate::system::open("https://go.microsoft.com/fwlink/p/?LinkId=2124703"); // WebView2 runtime
            }
        }
    }

    fn create(&mut self, page: &str) -> anyhow::Result<Window> {
        let dark = dark_mode();
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

        let requests = self.request_tx.clone();
        let (r, g, b) = if dark { (0x1c, 0x19, 0x17) } else { (0xfb, 0xf8, 0xf4) };
        // The page is loaded as a string, not via a custom protocol: wry's protocol handler holds the WebView2
        // environment, which kept Edge's browser processes (~360 MB) alive after the window closed.
        let webview = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_initialization_script(format!("window.__MEW_PAGE = {};", serde_json::to_string(page)?))
            .with_ipc_handler(move |request: Request<String>| {
                let _ = requests.send(request.into_body());
            })
            .with_html(PAGE)
            .with_background_color((r, g, b, 255))
            .with_devtools(cfg!(debug_assertions))
            .with_navigation_handler(|url| url.starts_with("about:blank") || url.starts_with("data:"))
            .build(&Handle(hwnd))?;
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
        CLOSE_REQUESTED.store(false, Ordering::Relaxed);
        Ok(Window { hwnd, webview })
    }

    /// Handles window housekeeping and answers queued page requests with `api`. Call after each message.
    pub fn pump(&mut self, api: &mut dyn FnMut(&str, &Value) -> anyhow::Result<Value>) {
        if CLOSE_REQUESTED.swap(false, Ordering::Relaxed)
            && let Some(window) = self.window.take()
        {
            let hwnd = window.hwnd;
            drop(window); // WebView first, then its parent window
            unsafe { DestroyWindow(hwnd) };
            end_webview_processes();
        }
        if THEME_CHANGED.swap(false, Ordering::Relaxed)
            && let Some(w) = &self.window
        {
            style_title_bar(w.hwnd, dark_mode());
        }
        while let Ok(raw) = self.requests.try_recv() {
            let Ok(request) = serde_json::from_str::<Value>(&raw) else { continue };
            let id = request["id"].as_i64().unwrap_or(0);
            let method = request["method"].as_str().unwrap_or_default();
            let reply = match api(method, &request["params"]) {
                Ok(data) => json!({ "id": id, "ok": true, "data": data }),
                Err(e) => {
                    log::warn!("ui request {method} failed: {e:#}");
                    json!({ "id": id, "ok": false, "error": e.to_string() })
                }
            };
            self.send(&reply);
        }
    }

    /// Sends an event to the page (new dictation, settings changed elsewhere, navigate).
    pub fn push(&self, event: &Value) {
        self.send(&json!({ "event": event }));
    }

    fn send(&self, message: &Value) {
        if let Some(w) = &self.window {
            let _ = w.webview.evaluate_script(&format!("window.__mew && window.__mew.receive({message})"));
        }
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
fn dark_mode() -> bool {
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
