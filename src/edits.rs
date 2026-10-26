//! Watches the text box Mew just typed into, so fixes the user makes can be learned (see `learn.rs`).
//! Only that box, only for 30 seconds, never password fields. Nothing leaves this PC.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationValuePattern,
    UIA_TextPatternId, UIA_ValuePatternId,
};

use crate::learn::{self, Fix};

const WATCH_FOR: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(1);
const SETTLE: Duration = Duration::from_millis(600); // let the paste land first
const MAX_TEXT: i32 = 200_000;

/// Bumped per dictation; an older watch stops early and judges what it has seen so far.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Starts watching the focused text box for fixes to `typed`; calls `on_fixes` if any are found.
pub fn watch(typed: String, on_fixes: impl FnOnce(Vec<Fix>) + Send + 'static) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = std::thread::Builder::new().name("edits".into()).spawn(move || match observe(&typed, generation) {
        Ok(fixes) if !fixes.is_empty() => on_fixes(fixes),
        Ok(_) => {}
        Err(e) => log::info!("couldn't watch the text box for edits: {e}"),
    });
}

#[derive(Clone, Copy)]
enum Source {
    Value,
    Text,
}

fn observe(typed: &str, generation: u64) -> windows::core::Result<Vec<Fix>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        std::thread::sleep(SETTLE);
        let element = uia.GetFocusedElement()?;
        if element.CurrentIsPassword()?.as_bool() {
            return Ok(Vec::new());
        }
        // Use whichever pattern shows the text Mew just typed (inputs expose Value, rich editors Text).
        let typed_norm = normalize(typed);
        let Some((source, before)) = [Source::Value, Source::Text]
            .into_iter()
            .find_map(|s| read(&element, s).filter(|t| t.contains(typed_norm.trim())).map(|t| (s, t)))
        else {
            return Ok(Vec::new());
        };
        let mut after = before.clone();
        let started = Instant::now();
        while started.elapsed() < WATCH_FOR && GENERATION.load(Ordering::SeqCst) == generation {
            std::thread::sleep(POLL);
            match read(&element, source) {
                Some(text) => after = text,
                None => break, // the box went away
            }
        }
        Ok(learn::fixes(&typed_norm, &before, &after))
    }
}

fn read(element: &IUIAutomationElement, source: Source) -> Option<String> {
    let text = unsafe {
        match source {
            Source::Value => element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()?
                .CurrentValue()
                .ok()?
                .to_string(),
            Source::Text => element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                .ok()?
                .DocumentRange()
                .ok()?
                .GetText(MAX_TEXT)
                .ok()?
                .to_string(),
        }
    };
    Some(normalize(&text))
}

/// Editors report line breaks and spaces their own way; compare on a common form.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\u{a0}', " ")
}
