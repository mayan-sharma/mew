//! Where Mew keeps its files, and the settings the tray menu changes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Models, settings, dictionary, history and logs. Kept apart from Velopack's install folder so updates never
/// touch it. `MEW_DATA_DIR` overrides it for development.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MEW_DATA_DIR") {
        return dir.into();
    }
    let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
    base.join("Mew Data")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn dictionary_path() -> PathBuf {
    data_dir().join("dictionary.txt")
}

/// llama-server ships next to mew.exe; `MEW_LLAMA_DIR` overrides it for development.
pub fn llama_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MEW_LLAMA_DIR") {
        return dir.into();
    }
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent().map(|p| p.join("llama")).unwrap_or_default()
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hotkey {
    RightAlt,
    RightCtrl,
    CapsLock,
    F8,
}

impl Hotkey {
    pub const ALL: [Hotkey; 4] = [Hotkey::RightAlt, Hotkey::RightCtrl, Hotkey::CapsLock, Hotkey::F8];

    pub fn vk(self) -> u32 {
        match self {
            Hotkey::RightAlt => 0xA5,  // VK_RMENU
            Hotkey::RightCtrl => 0xA3, // VK_RCONTROL
            Hotkey::CapsLock => 0x14,  // VK_CAPITAL
            Hotkey::F8 => 0x77,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Hotkey::RightAlt => "Right Alt",
            Hotkey::RightCtrl => "Right Ctrl",
            Hotkey::CapsLock => "Caps Lock",
            Hotkey::F8 => "F8",
        }
    }
}

/// How long dictated text stays in history. Stats (word counts, timings) are always kept.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeepHistory {
    Off,
    Week,
    Month,
    Quarter,
    Forever,
}

impl KeepHistory {
    pub const ALL: [KeepHistory; 5] =
        [KeepHistory::Off, KeepHistory::Week, KeepHistory::Month, KeepHistory::Quarter, KeepHistory::Forever];

    pub fn days(self) -> Option<i64> {
        match self {
            KeepHistory::Off => Some(0),
            KeepHistory::Week => Some(7),
            KeepHistory::Month => Some(30),
            KeepHistory::Quarter => Some(90),
            KeepHistory::Forever => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KeepHistory::Off => "Don't keep",
            KeepHistory::Week => "7 days",
            KeepHistory::Month => "30 days",
            KeepHistory::Quarter => "90 days",
            KeepHistory::Forever => "Forever",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub hotkey: Hotkey,
    /// Microphone name; `None` follows the Windows default.
    pub microphone: Option<String>,
    /// Clean up transcripts with the local LLM.
    pub cleanup: bool,
    /// How long to keep transcript text.
    pub history: KeepHistory,
    /// Watch the text box after typing and learn spelling fixes the user makes.
    pub learn: bool,
    /// First-run setup (autostart, dictionary, welcome) has happened.
    pub welcomed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: Hotkey::RightAlt,
            microphone: None,
            cleanup: true,
            history: KeepHistory::Month,
            learn: true,
            welcomed: false,
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(data_dir());
        if let Err(e) = std::fs::write(Self::path(), serde_json::to_string_pretty(self).unwrap_or_default()) {
            log::warn!("saving settings failed: {e}");
        }
    }
}

/// Terms to spell exactly, one per line; `#` starts a comment.
pub fn dictionary() -> Vec<String> {
    std::fs::read_to_string(dictionary_path())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

pub fn set_dictionary(terms: &[String]) {
    let mut text = String::from(
        "# Words Mew should always spell exactly like this, one per line.\n\
         # Names, product names, jargon. Changes apply to your next dictation.\n",
    );
    for term in terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        text.push_str(term);
        text.push('\n');
    }
    let _ = std::fs::create_dir_all(data_dir());
    if let Err(e) = std::fs::write(dictionary_path(), text) {
        log::warn!("saving dictionary failed: {e}");
    }
}

pub fn ensure_dictionary() {
    let path = dictionary_path();
    if !path.exists() {
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(
            path,
            "# Words Mew should always spell exactly like this, one per line.\n\
             # Names, product names, jargon. Changes apply to your next dictation.\n",
        );
    }
}
