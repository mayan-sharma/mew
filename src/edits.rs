//! Watches the text box Mew just typed into, so fixes the user makes can be learned (see `learn.rs`).
//! Only that box, only for 30 seconds, never password fields. Nothing leaves this computer.
//! Windows reads the box through UI Automation, macOS through the Accessibility API.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::learn::{self, Fix};

#[cfg_attr(windows, path = "edits/windows.rs")]
#[cfg_attr(target_os = "macos", path = "edits/macos.rs")]
mod platform;

const WATCH_FOR: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(1);
const SETTLE: Duration = Duration::from_millis(600); // let the paste land first
const MAX_TEXT: usize = 200_000;

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

fn observe(typed: &str, generation: u64) -> Result<Vec<Fix>> {
    std::thread::sleep(SETTLE);
    // Use whichever reading shows the text Mew just typed (on Windows, inputs expose a Value, rich editors Text).
    let typed_norm = normalize(typed);
    let Some((field, before)) = platform::focused()?.into_iter().find_map(|field| {
        let text = normalize(&field.read()?);
        text.contains(typed_norm.trim()).then_some((field, text))
    }) else {
        return Ok(Vec::new());
    };
    let mut after = before.clone();
    let started = Instant::now();
    while started.elapsed() < WATCH_FOR && GENERATION.load(Ordering::SeqCst) == generation {
        std::thread::sleep(POLL);
        match field.read() {
            Some(text) => after = normalize(&text),
            None => break, // the box went away
        }
    }
    Ok(learn::fixes(&typed_norm, &before, &after))
}

/// Editors report line breaks and spaces their own way; compare on a common form.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\u{a0}', " ")
}
