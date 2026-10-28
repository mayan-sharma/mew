//! Minimal file logger: `logs/mew.log`, rolled to `mew.old.log` past 1 MB. Dictated text is never logged.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

use log::{LevelFilter, Log, Metadata, Record};

use crate::config;

struct FileLogger {
    file: Mutex<File>,
    echo: bool,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let [year, month, day, hour, minute, second, ms] = local_time();
        let line = format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{ms:03} {:5} {}\n",
            record.level(),
            record.args()
        );
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
        }
        if self.echo {
            eprint!("{line}");
        }
    }

    fn flush(&self) {}
}

/// Year, month, day, hour, minute, second, millisecond.
#[cfg(windows)]
fn local_time() -> [u32; 7] {
    let t = unsafe {
        let mut t = std::mem::zeroed();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        t
    };
    [t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds].map(u32::from)
}

#[cfg(target_os = "macos")]
fn local_time() -> [u32; 7] {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() as libc::time_t;
    let t = unsafe {
        let mut t: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut t);
        t
    };
    let [y, mo, d, h, mi, s] =
        [t.tm_year + 1900, t.tm_mon + 1, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec].map(|v| v as u32);
    [y, mo, d, h, mi, s, now.subsec_millis()]
}

/// `echo` also prints log lines to the console (command-line runs).
pub fn init(echo: bool) {
    let dir = config::logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("mew.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = std::fs::rename(&path, dir.join("mew.old.log"));
    }
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = log::set_boxed_logger(Box::new(FileLogger { file: Mutex::new(file), echo }));
        log::set_max_level(LevelFilter::Info);
    }
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
}
