//! Small Windows integrations: start with Windows, app names, opening files and settings pages.

use std::collections::HashMap;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::overlay::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Mew";

pub fn autostart_enabled() -> bool {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    status == 0
}

pub fn set_autostart(enabled: bool) {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    let status = unsafe {
        if enabled {
            let exe = std::env::current_exe().unwrap_or_default();
            let command = wide(&format!("\"{}\"", exe.display()));
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr())
        }
    };
    if status != 0 && enabled {
        log::warn!("could not update autostart: error {status}");
    }
}

/// Friendly name of the app that owns a window ("Google Chrome"), from the exe's version info.
pub fn app_name(hwnd: HWND) -> String {
    static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    let Some(path) = exe_path(hwnd) else { return "Unknown app".into() };
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(name) = cache.get(&path) {
        return name.clone();
    }
    let stem = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = file_description(&path).filter(|d| !d.trim().is_empty()).unwrap_or(stem);
    cache.insert(path, name.clone());
    name
}

fn exe_path(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn file_description(path: &str) -> Option<String> {
    let wpath = wide(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(wpath.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wpath.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
            return None;
        }
        // Use the file's first declared language/codepage.
        let (mut ptr, mut len) = (std::ptr::null_mut(), 0u32);
        let translation = wide(r"\VarFileInfo\Translation");
        if VerQueryValueW(data.as_ptr().cast(), translation.as_ptr(), &mut ptr, &mut len) == 0 || len < 4 {
            return None;
        }
        let pair = std::slice::from_raw_parts(ptr as *const u16, 2);
        let key = wide(&format!(r"\StringFileInfo\{:04x}{:04x}\FileDescription", pair[0], pair[1]));
        if VerQueryValueW(data.as_ptr().cast(), key.as_ptr(), &mut ptr, &mut len) == 0 || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        Some(String::from_utf16_lossy(text).trim_end_matches('\0').trim().to_string())
    }
}

/// Opens a file, folder, or `ms-settings:` URI with its default handler.
pub fn open(target: &str) {
    let (verb, target) = (wide("open"), wide(target));
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}
