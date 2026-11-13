//! Voice commands: "Mew, open Chrome". A dictation that starts with "Mew" is a command; the local model picks one
//! action from a fixed list (its reply is held to a JSON schema) and Mew carries it out. The model only names
//! things: finding and launching an app is plain code, and nothing it says is ever run as a shell command.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{Command, apply};
use crate::config::{KeepHistory, Settings};
use crate::llm::Cleaner;
use crate::overlay::Overlay;
use crate::system;

/// How speech-to-text spells "Mew".
const WAKE: [&str; 4] = ["mew", "meow", "mu", "miu"];
const GREETINGS: [&str; 4] = ["hey", "hi", "ok", "okay"];
/// A command may have to load the model first (it only preloads on key press when tidy-up is on).
const START_TIMEOUT: Duration = Duration::from_secs(30);
const ALIASES: [(&str, &str); 3] =
    [("vs code", "visual studio code"), ("vscode", "visual studio code"), ("system preferences", "system settings")];

const SYSTEM: &str = "You turn a spoken command to Mew, a dictation app, into one action. Reply with JSON only.
- open_app: open any app on this computer, built-in ones (Terminal, Finder, Notes) included. \"app\" is its name as spoken.
- set_setting: switch a Mew setting on or off. cleanup: tidying dictated text (removing ums, fixing punctuation). learn: learning spellings from the user's edits. autostart: starting Mew at login.
- keep_history: how long Mew keeps transcripts: Off, Week (7 days), Month (30 days), Quarter (90 days) or Forever.
- open_page: open Mew's own window on its overview, history, dictionary or settings page.
- open_site: open a website in the browser. youtube, x (formerly Twitter), or rick_roll (play \"Never Gonna Give You Up\" on YouTube).
- none: anything else, including things Mew can't do.";

pub struct Request<'a> {
    pub text: &'a str,
    /// Said as "Mew, …" or "Hey Mew …": surely meant for Mew, not dictation that happens to start with "Mew".
    pub addressed: bool,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    OpenApp {
        app: String,
    },
    SetSetting {
        setting: Toggle,
        on: bool,
    },
    KeepHistory {
        keep: KeepHistory,
    },
    OpenPage {
        page: Page,
    },
    OpenSite {
        site: Site,
    },
    #[serde(rename = "none")]
    Nothing,
}

#[derive(Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Toggle {
    Cleanup,
    Learn,
    Autostart,
}

#[derive(Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    Overview,
    History,
    Dictionary,
    Settings,
}

#[derive(Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Site {
    Youtube,
    X,
    RickRoll,
}

impl Site {
    /// What the cat says and the address it opens.
    fn open(self) -> (&'static str, &'static str) {
        match self {
            Site::Youtube => ("Opening YouTube.", "https://www.youtube.com"),
            Site::X => ("Opening X.", "https://x.com"),
            Site::RickRoll => ("Never gonna give you up…", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
        }
    }
}

/// The command in a transcript that starts with "Mew", if any.
pub fn request(raw: &str) -> Option<Request<'_>> {
    let (mut word, mut rest) = split_word(raw.trim_start());
    let mut addressed = false;
    if GREETINGS.iter().any(|g| g.eq_ignore_ascii_case(word)) {
        (word, rest) = split_word(rest.trim_start_matches([' ', ',']));
        addressed = true;
    }
    if !WAKE.iter().any(|w| w.eq_ignore_ascii_case(word)) {
        return None;
    }
    addressed |= rest.starts_with([',', '.', '!', ':']);
    let text = rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | ':' | '-'));
    text.chars().any(char::is_alphanumeric).then_some(Request { text, addressed })
}

fn split_word(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !c.is_alphanumeric() && c != '\'').unwrap_or(s.len());
    s.split_at(end)
}

/// The action the model picks for a command.
pub fn plan(text: &str, cleaner: &Cleaner) -> Result<Action> {
    Ok(serde_json::from_value(cleaner.ask_json(&messages(text), &schema(), 80)?)?)
}

/// Carries out a command. False when it turned out to be dictation after all (no action, and not clearly said
/// to Mew), so the caller types it as usual.
pub fn run(
    request: &Request,
    cleaner: &Cleaner,
    settings: &Mutex<Settings>,
    overlay: &Overlay,
    pushes: &Sender<Value>,
) -> bool {
    if !cleaner.wait_ready(START_TIMEOUT) {
        if request.addressed {
            overlay.huh("I can follow commands once my tidy-up model is ready.");
        }
        return request.addressed;
    }
    let action = match plan(request.text, cleaner) {
        Ok(action) => action,
        Err(e) => {
            log::warn!("command failed: {e:#}");
            if request.addressed {
                overlay.huh("Oops, I couldn't work that out.");
            }
            return request.addressed;
        }
    };
    log::info!("command: {action:?}");
    let settings_changed = || {
        let _ = pushes.send(json!({ "type": "settings" }));
        system::wake();
    };
    match action {
        Action::Nothing if !request.addressed => return false,
        Action::Nothing => overlay.huh("I can't do that yet."),
        Action::OpenApp { app } => match find_app(&app, &installed_apps()) {
            Some((name, path)) => {
                overlay.say(&format!("Opening {name}."));
                system::open(&path.to_string_lossy());
            }
            None => overlay.huh(&format!("I couldn't find an app called “{app}”.")),
        },
        Action::SetSetting { setting, on } => {
            let command = match setting {
                Toggle::Cleanup => Command::SetCleanup(on),
                Toggle::Learn => Command::SetLearn(on),
                Toggle::Autostart => Command::SetAutostart(on),
            };
            apply(command, settings, overlay, false); // speaks for cleanup only
            match (setting, on) {
                (Toggle::Cleanup, _) => {}
                (Toggle::Learn, true) => overlay.say("I'll learn spellings from your edits."),
                (Toggle::Learn, false) => overlay.say("I'll stop learning from your edits."),
                (Toggle::Autostart, true) => overlay.say("I'll start when you log in."),
                (Toggle::Autostart, false) => overlay.say("I won't start at login."),
            }
            settings_changed();
        }
        Action::KeepHistory { keep } => {
            apply(Command::SetHistory(keep), settings, overlay, false);
            overlay.say(&match keep {
                KeepHistory::Off => "I won't keep your transcripts.".into(),
                KeepHistory::Forever => "I'll keep your transcripts forever.".into(),
                keep => format!("I'll keep your transcripts for {}.", keep.label()),
            });
            settings_changed();
        }
        Action::OpenPage { page } => {
            overlay.idle();
            let page = match page {
                Page::Overview => "overview",
                Page::History => "history",
                Page::Dictionary => "dictionary",
                Page::Settings => "settings",
            };
            let _ = pushes.send(json!({ "type": "open", "page": page }));
            system::wake();
        }
        Action::OpenSite { site } => {
            let (said, url) = site.open();
            overlay.say(said);
            system::open(url);
        }
    }
    true
}

/// Worked examples; the small model follows these far better than the rules alone.
fn examples() -> [(&'static str, Value); 13] {
    [
        ("open chrome", json!({ "action": "open_app", "app": "Chrome" })),
        ("can you launch spotify for me", json!({ "action": "open_app", "app": "Spotify" })),
        ("open the calculator", json!({ "action": "open_app", "app": "Calculator" })),
        ("open system settings", json!({ "action": "open_app", "app": "System Settings" })),
        ("stop tidying up my text", json!({ "action": "set_setting", "setting": "cleanup", "on": false })),
        ("turn on start at login", json!({ "action": "set_setting", "setting": "autostart", "on": true })),
        ("only keep my history for a week", json!({ "action": "keep_history", "keep": "Week" })),
        ("show me your dictionary", json!({ "action": "open_page", "page": "dictionary" })),
        ("go to youtube", json!({ "action": "open_site", "site": "youtube" })),
        ("open x", json!({ "action": "open_site", "site": "x" })),
        ("rick roll me", json!({ "action": "open_site", "site": "rick_roll" })),
        ("is the best app I've ever used", json!({ "action": "none" })),
        ("write an email to John", json!({ "action": "none" })),
    ]
}

fn messages(text: &str) -> Value {
    let mut msgs = vec![json!({ "role": "system", "content": SYSTEM })];
    for (said, action) in examples() {
        msgs.push(json!({ "role": "user", "content": said }));
        msgs.push(json!({ "role": "assistant", "content": action.to_string() }));
    }
    msgs.push(json!({ "role": "user", "content": text }));
    Value::Array(msgs)
}

/// One object shape per action. Keys are sent sorted, so "action" (first alphabetically) is written first.
fn schema() -> Value {
    let action = |name: &str, args: Value| {
        let mut properties = json!({ "action": { "const": name } });
        let mut required = vec![json!("action")];
        for (key, value) in args.as_object().into_iter().flatten() {
            properties[key] = value.clone();
            required.push(json!(key));
        }
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
    };
    json!({ "oneOf": [
        action("open_app", json!({ "app": { "type": "string", "minLength": 1, "maxLength": 60 } })),
        action("set_setting", json!({
            "setting": { "enum": ["cleanup", "learn", "autostart"] },
            "on": { "type": "boolean" },
        })),
        action("keep_history", json!({ "keep": { "enum": KeepHistory::ALL } })),
        action("open_page", json!({ "page": { "enum": ["overview", "history", "dictionary", "settings"] } })),
        action("open_site", json!({ "site": { "enum": ["youtube", "x", "rick_roll"] } })),
        action("none", json!({})),
    ]})
}

/// Apps that can be opened: name and the path to hand the OS. Start Menu shortcuts on Windows (Store apps without
/// one aren't found yet), `.app` bundles on macOS.
pub fn installed_apps() -> Vec<(String, PathBuf)> {
    let env = |var| std::env::var_os(var).map(PathBuf::from);
    let dirs: Vec<PathBuf> = if cfg!(windows) {
        [env("ProgramData"), env("APPDATA")]
            .into_iter()
            .flatten()
            .map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs"))
            .collect()
    } else {
        let mut dirs = vec![PathBuf::from("/Applications"), PathBuf::from("/System/Applications")];
        dirs.extend(env("HOME").map(|h| h.join("Applications")));
        dirs
    };
    let mut apps = Vec::new();
    if cfg!(target_os = "macos") {
        apps.push(("Finder".into(), PathBuf::from("/System/Library/CoreServices/Finder.app")));
    }
    for dir in dirs {
        collect_apps(&dir, 2, &mut apps);
    }
    apps
}

fn collect_apps(dir: &Path, depth: u32, apps: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for path in entries.flatten().map(|e| e.path()) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        let launchable = if cfg!(windows) { ext == "lnk" || ext == "url" } else { ext == "app" };
        if launchable {
            let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            if !name.to_lowercase().contains("uninstall") {
                apps.push((name, path));
            }
        } else if depth > 0 && path.is_dir() {
            collect_apps(&path, depth - 1, apps);
        }
    }
}

/// The best match for a spoken app name: the same name, then one holding every spoken word, shortest first
/// ("chrome" finds "Google Chrome", not "Google Chrome Canary").
pub fn find_app<'a>(spoken: &str, apps: &'a [(String, PathBuf)]) -> Option<&'a (String, PathBuf)> {
    let mut spoken = words(spoken).join(" ");
    if let Some((_, to)) = ALIASES.iter().find(|(from, _)| *from == spoken) {
        spoken = to.to_string();
    }
    let spoken: Vec<String> = spoken.split(' ').filter(|w| !w.is_empty()).map(String::from).collect();
    if spoken.is_empty() {
        return None;
    }
    apps.iter()
        .filter_map(|app| {
            let name = words(&app.0);
            let rank = if name == spoken {
                0
            } else if name.concat() == spoken.concat() {
                1 // "power point" for "PowerPoint"
            } else if spoken.iter().all(|w| name.contains(w)) {
                2
            } else if spoken.concat().len() >= 3 && name.concat().contains(&spoken.concat()) {
                3
            } else {
                return None;
            };
            Some(((rank, app.0.len()), app))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, app)| app)
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| !matches!(w.as_str(), "the" | "app" | "application"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_wake_word() {
        let r = request("Mew, open Chrome.").unwrap();
        assert_eq!((r.text, r.addressed), ("open Chrome.", true));
        let r = request("hey mew open chrome").unwrap();
        assert_eq!((r.text, r.addressed), ("open chrome", true));
        let r = request("Mew is the best app").unwrap();
        assert_eq!((r.text, r.addressed), ("is the best app", false));
        assert!(request("Mew.").is_none());
        assert!(request("Hey, how are you?").is_none());
        assert!(request("Mewtwo is a Pokémon").is_none());
        assert!(request("Let's open Chrome").is_none());
    }

    #[test]
    fn matches_spoken_app_names() {
        let apps: Vec<(String, PathBuf)> = [
            "Google Chrome Canary",
            "Google Chrome",
            "Microsoft PowerPoint",
            "Visual Studio Code",
            "System Settings",
            "Xcode",
        ]
        .iter()
        .map(|n| (n.to_string(), PathBuf::from(n)))
        .collect();
        let find = |spoken| find_app(spoken, &apps).map(|a| a.0.as_str());
        assert_eq!(find("Chrome"), Some("Google Chrome"));
        assert_eq!(find("chrome canary"), Some("Google Chrome Canary"));
        assert_eq!(find("power point"), Some("Microsoft PowerPoint"));
        assert_eq!(find("VS Code"), Some("Visual Studio Code"));
        assert_eq!(find("the settings app"), Some("System Settings"));
        assert_eq!(find("Photoshop"), None);
        assert_eq!(find("X"), None); // not "Xcode"
        assert_eq!(find("the app"), None);
    }

    #[test]
    fn examples_parse_as_actions() {
        for (said, action) in examples() {
            serde_json::from_value::<Action>(action).unwrap_or_else(|e| panic!("{said}: {e}"));
        }
        assert_eq!(
            serde_json::from_value::<Action>(json!({ "action": "keep_history", "keep": "Quarter" })).unwrap(),
            Action::KeepHistory { keep: KeepHistory::Quarter }
        );
    }
}
