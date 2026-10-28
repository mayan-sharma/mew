//! Types text by putting it on the clipboard and pasting (Ctrl+V on Windows, Cmd+V on macOS), then restores
//! whatever the user had copied. Both writes are kept out of clipboard history.

#[cfg_attr(windows, path = "paste/windows.rs")]
#[cfg_attr(target_os = "macos", path = "paste/macos.rs")]
mod platform;

pub use platform::{copy, paste};
