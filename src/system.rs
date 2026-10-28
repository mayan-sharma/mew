//! Small OS integrations: the main thread's event loop, one Mew at a time, starting at login, app names, and
//! opening files and settings pages.

#[cfg_attr(windows, path = "system/windows.rs")]
#[cfg_attr(target_os = "macos", path = "system/macos.rs")]
mod platform;

pub use platform::{
    app_name, autostart_enabled, first_instance, foreground_window, init, is_installed, open, run_loop, set_autostart,
    wake,
};
