use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::Duration;

use objc2_application_services::{AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFMachPort, CFRetained, CFRunLoop, CFString, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy,
    CGEventType, CGRequestListenEventAccess,
};

use super::{CAPTURE_ESCAPE, HELD, HOTKEY_VK, send};
use crate::app::AppEvent;

const ESCAPE: u32 = 0x35; // kVK_Escape

/// Set when another key goes down while the talk key is held: that's a shortcut, so apps must see the release.
static CHORD: AtomicBool = AtomicBool::new(false);
static TAP: AtomicPtr<CFMachPort> = AtomicPtr::new(std::ptr::null_mut());

/// Creates the event tap once macOS allows it, then runs the run loop it needs.
pub fn listen() {
    let mut asked = false;
    let tap = loop {
        if let Some(tap) = create_tap() {
            break tap;
        }
        if !asked {
            asked = true;
            log::warn!("can't watch the keyboard yet; asking for the Accessibility permission");
            ask_for_access();
            send(AppEvent::KeyUnavailable);
        }
        std::thread::sleep(Duration::from_secs(1));
    };
    if asked {
        log::info!("keyboard access granted");
    }
    TAP.store(CFRetained::as_ptr(&tap).as_ptr(), Ordering::Relaxed);
    let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0).expect("event tap run loop source");
    let run_loop = CFRunLoop::current().expect("hotkey thread run loop");
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&tap, true);
    CFRunLoop::run();
}

fn create_tap() -> Option<CFRetained<CFMachPort>> {
    let mask =
        [CGEventType::KeyDown, CGEventType::KeyUp, CGEventType::FlagsChanged].iter().fold(0, |m, t| m | 1 << t.0);
    unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            mask,
            Some(on_event),
            std::ptr::null_mut(),
        )
    }
}

/// Shows the system prompt for Accessibility (swallowing keys, pasting, reading text boxes all need it) or, once
/// that's granted, for Input Monitoring.
fn ask_for_access() {
    let prompt = CFDictionary::<CFString, CFBoolean>::from_slices(
        &[unsafe { kAXTrustedCheckOptionPrompt }],
        &[CFBoolean::new(true)],
    );
    if unsafe { AXIsProcessTrustedWithOptions(Some(prompt.as_opaque())) } {
        CGRequestListenEventAccess();
    }
}

unsafe extern "C-unwind" fn on_event(
    _: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    _: *mut c_void,
) -> *mut CGEvent {
    let pass = event.as_ptr();
    let swallow = std::ptr::null_mut();
    if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
        // macOS switches off a tap that seems stuck; switch it back on.
        if let Some(tap) = unsafe { TAP.load(Ordering::Relaxed).as_ref() } {
            CGEvent::tap_enable(tap, true);
        }
        return pass;
    }
    let event = Some(unsafe { event.as_ref() });
    let key = CGEvent::integer_value_field(event, CGEventField::KeyboardEventKeycode) as u32;
    if kind == CGEventType::FlagsChanged && key == HOTKEY_VK.load(Ordering::Relaxed) {
        let down = CGEvent::flags(event).0 & modifier_mask(key) != 0;
        if down && !HELD.swap(true, Ordering::Relaxed) {
            CHORD.store(false, Ordering::Relaxed);
            send(AppEvent::KeyDown);
        } else if !down && HELD.swap(false, Ordering::Relaxed) {
            send(AppEvent::KeyUp);
            if CHORD.load(Ordering::Relaxed) {
                return pass; // apps saw the shortcut, so they must see the key come up
            }
        }
        return swallow;
    }
    if kind == CGEventType::KeyDown {
        if key == ESCAPE && CAPTURE_ESCAPE.load(Ordering::Relaxed) {
            send(AppEvent::Escape);
            return swallow;
        }
        if HELD.load(Ordering::Relaxed) && !CHORD.swap(true, Ordering::Relaxed) {
            send(AppEvent::Chord);
        }
    }
    pass
}

/// The flag bit that's set while this modifier key is down (right-hand keys have their own device bits).
fn modifier_mask(key: u32) -> u64 {
    match key {
        0x3F => CGEventFlags::MaskSecondaryFn.0,
        0x3D => 0x40, // NX_DEVICERALTKEYMASK
        0x36 => 0x10, // NX_DEVICERCMDKEYMASK
        _ => 0,
    }
}
