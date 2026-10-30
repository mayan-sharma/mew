//! What the Mew window's page can ask for. Runs on the tray thread; every call is quick (SQLite, files, atomics).

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::app::{Command, apply};
use crate::config::{self, Hotkey, KeepHistory, Settings};
use crate::overlay::Overlay;
use crate::tray::Tray;
use crate::{history, models, paste, system, update};

/// The licenses file, relative to the executable: next to mew.exe, in Mew.app's Resources folder on macOS.
const NOTICES: &str = if cfg!(target_os = "macos") { "../Resources/NOTICE.txt" } else { "NOTICE.txt" };
const MIC_PRIVACY: &str = if cfg!(target_os = "macos") {
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
} else {
    "ms-settings:privacy-microphone"
};

pub struct Ctx<'a> {
    pub settings: &'a Arc<Mutex<Settings>>,
    pub overlay: &'a Overlay,
    pub tray: &'a mut Tray,
    pub devices: &'a Arc<Mutex<Vec<String>>>,
    pub pushes: &'a Sender<Value>,
}

pub fn handle(method: &str, params: &Value, ctx: &mut Ctx) -> Result<Value> {
    Ok(match method {
        "overview" => json!({
            "stats": history::stats()?,
            "recent": history::list("", None, 6)?,
            "talkKey": ctx.settings.lock().unwrap().hotkey.label(),
        }),
        "history" => {
            const PAGE: i64 = 60;
            let query = params["query"].as_str().unwrap_or_default();
            let items = history::list(query, params["before"].as_i64(), PAGE)?;
            json!({ "more": items.len() as i64 == PAGE, "items": items })
        }
        "forget" => {
            history::forget(params["id"].as_i64().context("id")?)?;
            Value::Null
        }
        "forgetAll" => {
            history::forget_all()?;
            Value::Null
        }
        "resetStats" => {
            history::reset()?;
            Value::Null
        }
        "settings" => settings_json(ctx),
        "set" => {
            let value = &params["value"];
            let command = match params["key"].as_str().unwrap_or_default() {
                "hotkey" => Command::SetHotkey(parse(value)?),
                "microphone" => Command::SetMicrophone(value.as_str().map(String::from)),
                "cleanup" => Command::SetCleanup(value.as_bool().context("bool")?),
                "autostart" => Command::SetAutostart(value.as_bool().context("bool")?),
                "history" => Command::SetHistory(parse(value)?),
                "learn" => Command::SetLearn(value.as_bool().context("bool")?),
                "autoUpdates" => Command::SetAutoUpdates(value.as_bool().context("bool")?),
                key => bail!("unknown setting {key}"),
            };
            apply(command, ctx.settings, ctx.overlay, true);
            ctx.tray.sync(&ctx.settings.lock().unwrap());
            settings_json(ctx)
        }
        "dictionary" => dictionary_json()?,
        "setDictionary" => {
            let terms: Vec<String> = serde_json::from_value(params["terms"].clone())?;
            config::set_dictionary(&terms);
            dictionary_json()?
        }
        "forgetLearned" => {
            history::forget_fix(params["heard"].as_str().context("heard")?)?;
            dictionary_json()?
        }
        "open" => {
            let target = match params["target"].as_str().unwrap_or_default() {
                "logs" => config::logs_dir().to_string_lossy().into_owned(),
                "data" => config::data_dir().to_string_lossy().into_owned(),
                "notices" => std::env::current_exe()?.with_file_name(NOTICES).to_string_lossy().into_owned(),
                "micPrivacy" => MIC_PRIVACY.into(),
                other => bail!("can't open {other}"),
            };
            system::open(&target);
            Value::Null
        }
        "copy" => {
            paste::copy(params["text"].as_str().unwrap_or_default())?;
            Value::Null
        }
        "checkUpdates" => {
            update::check(ctx.overlay, ctx.pushes, true);
            Value::Null
        }
        "installUpdate" => {
            update::install()?;
            Value::Null
        }
        other => bail!("unknown method {other}"),
    })
}

fn dictionary_json() -> Result<Value> {
    Ok(json!({ "terms": config::dictionary(), "learned": history::learned()? }))
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    Ok(serde_json::from_value(value.clone())?)
}

fn settings_json(ctx: &Ctx) -> Value {
    let s = ctx.settings.lock().unwrap().clone();
    json!({
        "hotkey": s.hotkey,
        "hotkeys": Hotkey::ALL.iter().map(|h| json!({ "id": h, "label": h.label() })).collect::<Vec<_>>(),
        "microphone": s.microphone,
        "microphones": *ctx.devices.lock().unwrap(),
        "cleanup": s.cleanup,
        "learn": s.learn,
        "autoUpdates": s.auto_updates,
        "updateReady": update::available(),
        "autostart": system::autostart_enabled(),
        "history": s.history,
        "historyOptions": KeepHistory::ALL.iter().map(|k| json!({ "id": k, "label": k.label() })).collect::<Vec<_>>(),
        "version": env!("CARGO_PKG_VERSION"),
        "models": [
            { "name": "Parakeet TDT 0.6B v3", "role": "Speech to text", "ready": models::SPEECH.is_ready() },
            { "name": "Qwen3.5 2B", "role": "Tidy-up", "ready": models::CLEANUP.is_ready() },
        ],
    })
}
