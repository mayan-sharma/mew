//! Mew's on-screen presence: a per-pixel-alpha layered window, bottom center of the active monitor.
//! Never takes focus, lets clicks through, hides while a fullscreen app is in front.

use std::collections::VecDeque;
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

use crate::cat::{self, Mode, Renderer, Scene};

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
        let shared = state.clone();
        std::thread::Builder::new()
            .name("overlay".into())
            .spawn(move || Window::run(shared, level))
            .expect("spawn overlay thread");
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

struct Window {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    pos: (i32, i32),
    size: (i32, i32),
    scale: f32,
}

impl Window {
    fn run(state: Arc<Mutex<State>>, level: impl Fn() -> f32) {
        let renderer = Renderer::new();
        let mut win = Window::create();
        let mut levels: VecDeque<f32> = VecDeque::with_capacity(40);
        let mut pill_w = Mode::Idle.pill_width();
        let mut last: Option<(Mode, String, Option<u32>, bool, u32)> = None;
        let mut visible = false; // created hidden; the first frame shows it
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
                if visible {
                    beat.tick(2);
                    unsafe {
                        ShowWindow(win.hwnd, SW_HIDE);
                        ShowWindow(win.hwnd, SW_SHOWNOACTIVATE);
                    }
                }
            }
            let scene = {
                let mut s = state.lock().unwrap();
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
                    last = None;
                }
                let mut text = s.text.clone();
                if s.mode == Mode::Idle && text.is_empty() && win.hovered(pill_w) {
                    text = s.hover_tip.clone();
                }
                Scene {
                    mode: s.mode,
                    t: (now - s.since).as_secs_f32(),
                    pill_w,
                    levels: Vec::new(),
                    locked: s.locked,
                    text,
                    progress: s.progress,
                }
            };

            // Get out of the way of games, videos and presentations while idle.
            let hide = scene.mode == Mode::Idle && scene.text.is_empty() && fullscreen_app();
            if hide == visible {
                beat.tick(2);
                unsafe { ShowWindow(win.hwnd, if hide { SW_HIDE } else { SW_SHOWNOACTIVATE }) };
                visible = !hide;
            }

            if scene.mode == Mode::Listening {
                if levels.len() == levels.capacity() {
                    levels.pop_front();
                }
                levels.push_back((level() * 18.0).powf(0.7).min(1.0));
            } else {
                levels.clear();
            }
            let target = scene.mode.pill_width();
            pill_w = if (target - pill_w).abs() < 0.5 { target } else { pill_w + (target - pill_w) * 0.35 };
            let animating = matches!(scene.mode, Mode::Listening | Mode::Thinking) || pill_w != target;
            let key = (
                scene.mode,
                scene.text.clone(),
                scene.progress.map(|p| (p * 1000.0) as u32),
                scene.locked,
                pill_w.to_bits(),
            );
            if visible && (animating || last.as_ref() != Some(&key)) {
                let scene = Scene { pill_w, levels: levels.iter().copied().collect(), ..scene };
                beat.tick(1);
                win.blit(&renderer.render(&scene, win.scale));
                last = Some(key);
            }
            std::thread::sleep(Duration::from_millis(if animating { 1000 / FPS } else { 100 }));
        }
    }

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
            }
        }
    }

    /// Bottom center of the work area on the monitor with the foreground window, at that monitor's DPI.
    fn place(&mut self) {
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

    fn blit(&self, pixmap: &tiny_skia::Pixmap) {
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

    fn hovered(&self, pill_w: f32) -> bool {
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

fn fullscreen_app() -> bool {
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
