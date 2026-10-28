use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

use anyhow::Context;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSColor, NSScreen, NSWindow,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, ns_string};
use raw_window_handle::{AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::{WebView, WebViewBuilder};

use super::CLOSE_REQUESTED;

const SIZE: (f64, f64) = (1040.0, 720.0);
const MIN_SIZE: (f64, f64) = (760.0, 540.0);

pub struct Window {
    window: Retained<NSWindow>,
    _delegate: Retained<Delegate>,
    pub webview: WebView,
}

/// The window's content view, which wry swaps for its own.
struct Handle(NonNull<c_void>);

impl HasWindowHandle for Handle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(self.0))) })
    }
}

impl Window {
    pub fn create(builder: WebViewBuilder, dark: bool) -> anyhow::Result<Self> {
        let mtm = MainThreadMarker::new().context("the Mew window opens on the main thread")?;
        let area = NSScreen::mainScreen(mtm).map(|s| s.visibleFrame().size);
        let size = area.map_or(SIZE, |a| (SIZE.0.min(a.width), SIZE.1.min(a.height)));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::ZERO, NSSize::new(size.0, size.1)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(ns_string!("Mew"));
        window.setContentMinSize(NSSize::new(MIN_SIZE.0, MIN_SIZE.1));
        let (r, g, b) = super::background(dark);
        let color = |c: u8| f64::from(c) / 255.0;
        window.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(color(r), color(g), color(b), 1.0)));
        window.center();
        let delegate = Delegate::new(mtm);
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let view = window.contentView().context("window has no content view")?;
        let webview = builder.build(&Handle(NonNull::from(&*view).cast()))?;
        let this = Self { window, _delegate: delegate, webview };
        this.focus();
        Ok(this)
    }

    pub fn focus(&self) {
        let app = NSApplication::sharedApplication(MainThreadMarker::from(&*self.window));
        app.unhideWithoutActivation();
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        self.window.makeKeyAndOrderFront(None);
        if app.respondsToSelector(sel!(activate)) {
            app.activate(); // macOS 14+
        } else {
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }
    }

    /// AppKit follows the system theme on its own.
    pub fn follow_theme(&self) {}

    pub fn close(self) {
        let Self { window, _delegate, webview } = self;
        drop(webview); // the web view first, then its window
        window.setDelegate(None); // windows don't retain their delegate
        window.close();
        // Hand the keyboard back to the app you were in, like closing a window does on Windows.
        NSApplication::sharedApplication(MainThreadMarker::from(&*window)).hide(None);
        log::info!("Mew window closed");
    }
}

/// The system appearance (System Settings › Appearance).
pub fn dark_mode() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    let (light, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    appearance
        .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[light, dark]))
        .is_some_and(|name| &*name == dark)
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Delegate doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MewWindowDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            CLOSE_REQUESTED.store(true, Ordering::Relaxed); // the main loop tears down the web view
            false
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}
