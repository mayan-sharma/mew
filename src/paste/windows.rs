//! Both writes are hidden from Win+V history and cloud clipboard.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CONTROL, VK_V,
};

const CF_UNICODETEXT: u32 = 13;
/// GDI-handle formats that can't be copied as memory; Windows re-synthesizes them from the others.
const NON_MEMORY_FORMATS: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];
/// Time the target app gets to read the clipboard before it's restored.
const RESTORE_DELAY: Duration = Duration::from_millis(300);

type Snapshot = BTreeMap<u32, Vec<u8>>;

pub fn paste(text: &str) -> Result<()> {
    let saved = snapshot().ok();
    let utf16: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    write(&BTreeMap::from([(CF_UNICODETEXT, utf16)]))?;
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let sent = ctrl_v();
    if let Some(saved) = saved {
        std::thread::sleep(RESTORE_DELAY);
        // Skip if something else was copied in the meantime.
        if unsafe { GetClipboardSequenceNumber() } == sequence
            && let Err(e) = write(&saved)
        {
            log::warn!("restoring the clipboard failed: {e:#}"); // the text was still typed
        }
    }
    if !sent {
        bail!("Windows blocked the keystroke");
    }
    Ok(())
}

/// Puts text on the clipboard for the user (from the Mew window's Copy button). This one may enter Win+V history.
pub fn copy(text: &str) -> Result<()> {
    let utf16: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    let _open = Clipboard::open()?;
    unsafe {
        EmptyClipboard();
        let handle = GlobalAlloc(GMEM_MOVEABLE, utf16.len());
        if handle.is_null() {
            bail!("out of memory");
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), GlobalLock(handle) as *mut u8, utf16.len());
        GlobalUnlock(handle);
        if SetClipboardData(CF_UNICODETEXT, handle as _).is_null() {
            GlobalFree(handle);
            bail!("clipboard rejected the text");
        }
    }
    Ok(())
}

/// False if Windows didn't accept the keystrokes.
fn ctrl_v() -> bool {
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) == inputs.len() as u32 }
}

struct Clipboard;

impl Clipboard {
    fn open() -> Result<Self> {
        for _ in 0..50 {
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(Self);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        bail!("clipboard is busy")
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn snapshot() -> Result<Snapshot> {
    let _open = Clipboard::open()?;
    let mut items = Snapshot::new();
    let mut format = 0;
    loop {
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if NON_MEMORY_FORMATS.contains(&format) {
            continue;
        }
        unsafe {
            let handle = GetClipboardData(format) as HGLOBAL;
            if handle.is_null() {
                continue;
            }
            let ptr = GlobalLock(handle) as *const u8;
            if !ptr.is_null() {
                items.insert(format, std::slice::from_raw_parts(ptr, GlobalSize(handle)).to_vec());
                GlobalUnlock(handle);
            }
        }
    }
    Ok(items)
}

fn write(items: &Snapshot) -> Result<()> {
    let _open = Clipboard::open()?;
    unsafe { EmptyClipboard() };
    let zero = 0u32.to_le_bytes().to_vec();
    let private =
        ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"]
            .map(|name| (register(name), zero.clone()));
    for (format, data) in items.iter().map(|(f, d)| (*f, d.clone())).chain(private) {
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1));
            if handle.is_null() {
                continue;
            }
            let ptr = GlobalLock(handle) as *mut u8;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            GlobalUnlock(handle);
            if SetClipboardData(format, handle as _).is_null() {
                GlobalFree(handle);
            }
        }
    }
    Ok(())
}

fn register(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
}
