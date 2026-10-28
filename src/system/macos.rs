use std::fs::File;
use std::os::fd::AsRawFd;
use std::sync::OnceLock;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSEvent, NSEventMask, NSEventModifierFlags,
    NSEventType, NSRunningApplication, NSWorkspace,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSObject, NSObjectProtocol, NSPoint};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use tray_icon::menu::{Menu, PredefinedMenuItem, Submenu};

use crate::config;

/// False if another Mew runs with this data folder. (Opening Mew.app again doesn't start a second copy: macOS sends
/// the running one a reopen event, which opens its window.)
pub fn first_instance() -> bool {
    static LOCK: OnceLock<File> = OnceLock::new();
    let _ = std::fs::create_dir_all(config::data_dir());
    let Ok(file) = File::create(config::data_dir().join("mew.lock")) else { return true };
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return false;
    }
    let _ = LOCK.set(file); // held until Mew exits
    true
}

/// Process-wide setup, on the main thread before any window exists: no Dock icon, the reopen handler, and an Edit
/// menu so the Mew window's text boxes get Cmd+C, Cmd+V and friends.
pub fn init() {
    let mtm = MainThreadMarker::new().expect("init runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    std::mem::forget(delegate); // the app only holds its delegate weakly
    if let Err(e) = main_menu() {
        log::warn!("main menu failed: {e}");
    }
    app.finishLaunching();
}

fn main_menu() -> tray_icon::menu::Result<()> {
    let separator = PredefinedMenuItem::separator();
    let menu = Menu::with_items(&[
        &Submenu::with_items(
            "Mew",
            true,
            &[&PredefinedMenuItem::hide(None), &separator, &PredefinedMenuItem::quit(None)],
        )?,
        &Submenu::with_items(
            "Edit",
            true,
            &[
                &PredefinedMenuItem::undo(None),
                &PredefinedMenuItem::redo(None),
                &separator,
                &PredefinedMenuItem::cut(None),
                &PredefinedMenuItem::copy(None),
                &PredefinedMenuItem::paste(None),
                &PredefinedMenuItem::select_all(None),
            ],
        )?,
        &Submenu::with_items(
            "Window",
            true,
            &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::close_window(None)],
        )?,
    ])?;
    menu.init_for_nsapp();
    std::mem::forget(menu); // the app's menu for as long as Mew runs
    Ok(())
}

/// The main thread's event loop, which the tray, the overlay and the Mew window need. Calls `tick` after each event
/// until it returns false.
pub fn run_loop(mut tick: impl FnMut() -> bool) {
    let app = NSApplication::sharedApplication(MainThreadMarker::new().expect("run_loop runs on the main thread"));
    let forever = NSDate::distantFuture();
    loop {
        let more = autoreleasepool(|_| {
            let mode = unsafe { NSDefaultRunLoopMode };
            let event =
                app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&forever), mode, true);
            crate::watchdog::MAIN.tick(1);
            if let Some(event) = event {
                app.sendEvent(&event);
            }
            tick()
        });
        if !more {
            return;
        }
    }
}

/// Wakes `run_loop` from another thread, so it handles queued work.
pub fn wake() {
    dispatch2::DispatchQueue::main().exec_async(|| {
        let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
        let event =
            NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                NSEventType::ApplicationDefined,
                NSPoint::ZERO,
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                0,
                0,
                0,
            );
        if let Some(event) = event {
            NSApplication::sharedApplication(mtm).postEvent_atStart(&event, false);
        }
    });
}

/// The app you're typing in, by process id.
pub fn foreground_window() -> isize {
    autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace().frontmostApplication().map_or(0, |app| app.processIdentifier() as isize)
    })
}

/// Friendly name of an app ("Safari"), from its process id.
pub fn app_name(window: isize) -> String {
    autoreleasepool(|_| {
        NSRunningApplication::runningApplicationWithProcessIdentifier(window as libc::pid_t)
            .and_then(|app| app.localizedName())
            .map_or_else(|| "Unknown app".into(), |name| name.to_string())
    })
}

/// Running from Mew.app, as opposed to a dev build.
pub fn is_installed() -> bool {
    std::env::current_exe().is_ok_and(|exe| exe.ancestors().any(|p| p.extension().is_some_and(|e| e == "app")))
}

/// Mew starts at login only if its login item is registered and switched on in System Settings › General ›
/// Login Items.
pub fn autostart_enabled() -> bool {
    autoreleasepool(|_| unsafe { SMAppService::mainAppService().status() } == SMAppServiceStatus::Enabled)
}

pub fn set_autostart(enabled: bool) {
    let result = autoreleasepool(|_| unsafe {
        let service = SMAppService::mainAppService();
        let result = if enabled { service.registerAndReturnError() } else { service.unregisterAndReturnError() };
        result.map_err(|e| e.localizedDescription().to_string())
    });
    if let Err(e) = result
        && enabled
    {
        log::warn!("could not update autostart: {e}");
    }
}

/// Opens a file, folder, or `x-apple.systempreferences:` URL with its default handler.
pub fn open(target: &str) {
    let target = target.to_string();
    std::thread::spawn(move || std::process::Command::new("/usr/bin/open").arg(target).status());
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and AppDelegate doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MewAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        /// Opening Mew.app while it runs (Finder, Spotlight, Launchpad) opens the Mew window.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            crate::watchdog::request_open();
            false
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}
