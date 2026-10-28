//! A borderless, non-activating panel on every Space, just above the Dock. AppKit windows belong to the main
//! thread, so the animation runs there on a run loop timer, which keeps firing while a menu is open.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSPanel, NSScreen, NSStatusWindowLevel, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFData, CFRetained, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext, CFType,
    kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::{CALayer, CATransaction};

use super::{Animator, State};
use crate::cat;

pub fn start<L: Fn() -> f32 + Send + 'static>(state: Arc<Mutex<State>>, level: L) {
    let anim = Box::new(Animator::new(state, level, Window::create()));
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: Box::into_raw(anim).cast(), // lives as long as Mew
        retain: None,
        release: None,
        copyDescription: None,
    };
    // Repeats hourly in name only: every frame sets the next fire date.
    let timer =
        unsafe { CFRunLoopTimer::new(None, CFAbsoluteTimeGetCurrent(), 3600.0, 0, 0, Some(frame::<L>), &mut context) };
    let main = CFRunLoop::main().expect("main run loop");
    main.add_timer(timer.as_deref(), unsafe { kCFRunLoopCommonModes });
}

unsafe extern "C-unwind" fn frame<L: Fn() -> f32>(timer: *mut CFRunLoopTimer, info: *mut c_void) {
    let anim = unsafe { &mut *info.cast::<Animator<L>>() };
    crate::watchdog::OVERLAY.tick(0);
    let wait = anim.frame();
    if let Some(timer) = unsafe { timer.as_ref() } {
        timer.set_next_fire_date(CFAbsoluteTimeGetCurrent() + wait.as_secs_f64());
    }
}

pub struct Window {
    panel: Retained<NSPanel>,
    layer: Retained<CALayer>,
    pub size: (i32, i32),
    pub scale: f32,
    pub visible: bool,
}

impl Window {
    fn create() -> Self {
        let mtm = MainThreadMarker::new().expect("the overlay lives on the main thread");
        let rect = NSRect::new(NSPoint::ZERO, NSSize::new(cat::CANVAS_W.into(), cat::CANVAS_H.into()));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setIgnoresMouseEvents(true);
        panel.setLevel(NSStatusWindowLevel);
        panel.setHidesOnDeactivate(false);
        panel.setCanHide(false); // stays when closing the Mew window hides the app
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        // A layer-hosting view: AppKit leaves the layer's contents to us.
        let layer = CALayer::new();
        let view = panel.contentView().expect("panel content view");
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        Self { panel, layer, size: (0, 0), scale: 1.0, visible: false }
    }

    /// Bottom center of the screen you're working on (the one with the focused window), at its scale.
    pub fn place(&mut self) {
        let mtm = MainThreadMarker::from(&*self.panel);
        let Some(screen) = NSScreen::mainScreen(mtm) else { return };
        let area = screen.visibleFrame();
        let scale = screen.backingScaleFactor() as f32;
        let (w, h) = (f64::from(cat::CANVAS_W), f64::from(cat::CANVAS_H));
        let origin = NSPoint::new(area.origin.x + (area.size.width - w) / 2.0, area.origin.y + 6.0);
        self.panel.setFrame_display(NSRect::new(origin, NSSize::new(w, h)), false);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.layer.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(w, h)));
        self.layer.setContentsScale(scale.into());
        CATransaction::commit();
        self.scale = scale;
        self.size = ((cat::CANVAS_W * scale).round() as i32, (cat::CANVAS_H * scale).round() as i32);
    }

    pub fn show(&mut self, visible: bool) {
        if visible {
            self.panel.orderFrontRegardless();
        } else {
            self.panel.orderOut(None);
        }
        self.visible = visible;
    }

    pub fn blit(&self, pixmap: &tiny_skia::Pixmap) {
        let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
        if (w, h) != (self.size.0 as usize, self.size.1 as usize) {
            return;
        }
        // tiny-skia's premultiplied RGBA is what CoreGraphics calls premultiplied-last.
        let provider = CGDataProvider::with_cf_data(Some(&CFData::from_bytes(pixmap.data())));
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
        let image: Option<CFRetained<CGImage>> = unsafe {
            CGImage::new(
                w,
                h,
                8,
                32,
                w * 4,
                space.as_deref(),
                CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0),
                provider.as_deref(),
                std::ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        };
        let Some(image) = image else { return };
        let contents: &CFType = &image;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe { self.layer.setContents(Some(AsRef::<AnyObject>::as_ref(contents))) };
        CATransaction::commit();
    }

    pub fn hovered(&self, pill_w: f32) -> bool {
        let at = NSEvent::mouseLocation();
        let frame = self.panel.frame();
        let cx = frame.origin.x + frame.size.width / 2.0;
        let top = frame.origin.y + f64::from(4.0 + cat::PILL_H_IDLE + 8.0); // include the ears
        (at.x - cx).abs() <= f64::from(pill_w) / 2.0 && at.y >= frame.origin.y && at.y <= top
    }
}

/// Fullscreen apps get their own Space; the panel joins it so dictation feedback still shows there.
pub fn fullscreen_app() -> bool {
    false
}
