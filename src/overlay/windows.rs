//! A per-pixel-alpha layered window on its own thread, bottom center of the active monitor at its DPI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetMonitorInfoW, HBITMAP, HDC, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromWindow, SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetCursorPos, GetForegroundWindow,
    HWND_TOPMOST, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SetWindowPos, ShowWindow, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use super::{Animator, State};
use crate::cat;

pub fn start(state: Arc<Mutex<State>>, level: impl Fn() -> f32 + Send + 'static) {
    std::thread::Builder::new().name("overlay".into()).spawn(move || run(state, level)).expect("spawn overlay thread");
}

fn run(state: Arc<Mutex<State>>, level: impl Fn() -> f32) {
    let mut anim = Animator::new(state, level, Window::create());
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let mut switched_at: Option<Instant> = None;
    let beat = &crate::watchdog::OVERLAY;
    loop {
        beat.tick(0);
        pump_messages();
        let now = Instant::now();
        // After Alt-Tab, DWM can start drawing this layered window opaque (a black box around the cat).
        // Only a hide/show resets it, so do one once the switch has settled.
        if FOREGROUND_CHANGED.swap(false, Ordering::Relaxed) {
            switched_at = Some(now);
        }
        if switched_at.is_some_and(|t| now - t > Duration::from_millis(300)) {
            switched_at = None;
            if anim.win.visible {
                beat.tick(2);
                unsafe {
                    ShowWindow(anim.win.hwnd, SW_HIDE);
                    ShowWindow(anim.win.hwnd, SW_SHOWNOACTIVATE);
                }
            }
        }
        std::thread::sleep(anim.frame());
    }
}

pub struct Window {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    pos: (i32, i32),
    pub size: (i32, i32),
    pub scale: f32,
    pub visible: bool,
}

impl Window {
    fn create() -> Self {
        unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let class = wide("MewOverlay");
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(DefWindowProcW),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc);
            let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
            let title = wide("Mew");
            let hwnd = CreateWindowExW(
                ex,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            );
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            Self {
                hwnd,
                dc,
                bitmap: std::ptr::null_mut(),
                bits: std::ptr::null_mut(),
                pos: (0, 0),
                size: (0, 0),
                scale: 1.0,
                visible: false,
            }
        }
    }

    /// Bottom center of the work area on the monitor with the foreground window, at that monitor's DPI.
    pub fn place(&mut self) {
        unsafe {
            let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(monitor, &mut info);
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale = dpi_x as f32 / 96.0;
            let size = ((cat::CANVAS_W * scale).round() as i32, (cat::CANVAS_H * scale).round() as i32);
            let work: RECT = info.rcWork;
            self.pos = ((work.left + work.right - size.0) / 2, work.bottom - size.1 - (6.0 * scale).round() as i32);
            if size != self.size {
                self.resize(size);
            }
            self.scale = scale;
            SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    pub fn show(&mut self, visible: bool) {
        unsafe { ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
        self.visible = visible;
    }

    fn resize(&mut self, size: (i32, i32)) {
        unsafe {
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size.0,
                biHeight: -size.1, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut bits = std::ptr::null_mut();
            self.bitmap = CreateDIBSection(self.dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            SelectObject(self.dc, self.bitmap);
            self.bits = bits as *mut u8;
            self.size = size;
        }
    }

    pub fn blit(&self, pixmap: &tiny_skia::Pixmap) {
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        if self.bits.is_null() || pixmap.width() as usize != w || pixmap.height() as usize != h {
            return;
        }
        // tiny-skia is premultiplied RGBA; UpdateLayeredWindow wants premultiplied BGRA.
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, w * h * 4) };
        for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(pixmap.data().as_chunks::<4>().0) {
            *d = [s[2], s[1], s[0], s[3]];
        }
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let (pos, size, origin) =
            (POINT { x: self.pos.0, y: self.pos.1 }, SIZE { cx: self.size.0, cy: self.size.1 }, POINT { x: 0, y: 0 });
        unsafe {
            UpdateLayeredWindow(self.hwnd, std::ptr::null_mut(), &pos, &size, self.dc, &origin, 0, &blend, ULW_ALPHA)
        };
    }

    pub fn hovered(&self, pill_w: f32) -> bool {
        let mut pt = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut pt) };
        let cx = self.pos.0 as f32 + self.size.0 as f32 / 2.0;
        let top = self.pos.1 as f32 + (cat::CANVAS_H - 4.0 - cat::PILL_H_IDLE - 8.0) * self.scale; // include the ears
        (pt.x as f32 - cx).abs() <= pill_w / 2.0 * self.scale && pt.y as f32 >= top && pt.y < self.pos.1 + self.size.1
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

static FOREGROUND_CHANGED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    FOREGROUND_CHANGED.store(true, Ordering::Relaxed);
}

pub fn fullscreen_app() -> bool {
    let mut state = 0;
    unsafe { SHQueryUserNotificationState(&mut state) };
    matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
}

fn pump_messages() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
