//! Minimal file logger: `logs/mew.log`, rolled to `mew.old.log` past 1 MB. Dictated text is never logged.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

use log::{LevelFilter, Log, Metadata, Record};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

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
        let t = unsafe {
            let mut t = std::mem::zeroed();
            GetLocalTime(&mut t);
            t
        };
        let line = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {:5} {}\n",
            t.wYear,
            t.wMonth,
            t.wDay,
            t.wHour,
            t.wMinute,
            t.wSecond,
            t.wMilliseconds,
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
