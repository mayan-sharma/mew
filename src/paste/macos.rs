//! Both writes are marked transient and concealed (nspasteboard.org), so clipboard managers skip them.

use std::time::Duration;

use anyhow::{Result, bail};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardType, NSPasteboardTypeString, NSPasteboardWriting};
use objc2_application_services::AXIsProcessTrusted;
use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation};
use objc2_foundation::{NSArray, NSCopying, NSData, NSString};

/// Time the target app gets to read the clipboard before it's restored.
const RESTORE_DELAY: Duration = Duration::from_millis(300);
const KEY_V: u16 = 0x09; // kVK_ANSI_V
const PRIVATE: [&str; 2] = ["org.nspasteboard.TransientType", "org.nspasteboard.ConcealedType"];

/// Every clipboard item as (type, data) pairs.
type Snapshot = Vec<Vec<(Retained<NSPasteboardType>, Retained<NSData>)>>;

pub fn paste(text: &str) -> Result<()> {
    // Without it, the keystroke below is silently dropped.
    if !unsafe { AXIsProcessTrusted() } {
        bail!("no Accessibility permission");
    }
    autoreleasepool(|_| {
        let board = NSPasteboard::generalPasteboard();
        let saved = snapshot(&board);
        let kind = unsafe { NSPasteboardTypeString }.copy();
        write(&board, &vec![vec![(kind, NSData::with_bytes(text.as_bytes()))]]);
        let count = board.changeCount();
        cmd_v();
        std::thread::sleep(RESTORE_DELAY);
        // Skip if something else was copied in the meantime.
        if board.changeCount() == count {
            write(&board, &saved);
        }
    });
    Ok(())
}

/// Puts text on the clipboard for the user (from the Mew window's Copy button). This one may enter clipboard history.
pub fn copy(text: &str) -> Result<()> {
    autoreleasepool(|_| {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        if !board.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString }) {
            bail!("clipboard rejected the text");
        }
        Ok(())
    })
}

fn cmd_v() {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
    // Command plus the left Command key's own bit, which some apps check.
    let flags = CGEventFlags(CGEventFlags::MaskCommand.0 | 0x08);
    for down in [true, false] {
        if let Some(event) = CGEvent::new_keyboard_event(source.as_deref(), KEY_V, down) {
            CGEvent::set_flags(Some(&event), flags);
            CGEvent::post(CGEventTapLocation::AnnotatedSessionEventTap, Some(&event));
        }
    }
}

fn snapshot(board: &NSPasteboard) -> Snapshot {
    let Some(items) = board.pasteboardItems() else { return Vec::new() };
    items
        .iter()
        .map(|item| item.types().iter().filter_map(|kind| Some((kind.clone(), item.dataForType(&kind)?))).collect())
        .collect()
}

fn write(board: &NSPasteboard, items: &Snapshot) {
    let private: Vec<Retained<NSString>> = PRIVATE.map(NSString::from_str).into();
    let empty = NSData::new();
    let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = items
        .iter()
        .map(|entries| {
            let item = NSPasteboardItem::new();
            for (kind, data) in entries {
                item.setData_forType(data, kind);
            }
            for kind in &private {
                item.setData_forType(&empty, kind);
            }
            ProtocolObject::from_retained(item)
        })
        .collect();
    board.clearContents();
    board.writeObjects(&NSArray::from_retained_slice(&objects));
}
