//! Updates from GitHub Releases. On Windows, Velopack downloads them in the background; a downloaded update is applied
//! the next time Mew starts, or right away from Settings. The macOS zip isn't a Velopack install, so there Mew only
//! says a new version is out.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateManager};

use crate::overlay::Overlay;
use crate::system;

const FEED: &str = "https://github.com/mayan-sharma/mew";
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/mayan-sharma/mew/releases/latest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// The newer version waiting for the user, once Mew has told them about it.
static AVAILABLE: Mutex<Option<String>> = Mutex::new(None);
/// A check is running. Velopack's downloads have no timeout, so on a slow connection one can outlast the next check.
static CHECKING: AtomicBool = AtomicBool::new(false);

pub fn available() -> Option<String> {
    AVAILABLE.lock().unwrap().clone()
}

/// `manual` checks (from Settings) always report back; automatic ones only speak up when there's news.
pub fn check(overlay: &Overlay, pushes: &Sender<Value>, manual: bool) {
    if !system::is_installed() {
        if manual {
            overlay.say("Updates work once Mew is installed.");
        }
        return;
    }
    if CHECKING.swap(true, Ordering::SeqCst) {
        if manual {
            overlay.say("I'm already on it. Updates download in the background.");
        }
        return;
    }
    let (overlay, pushes) = (overlay.clone(), pushes.clone());
    std::thread::spawn(move || {
        let result = if cfg!(target_os = "macos") { newest_release() } else { download_update(&overlay, manual) };
        CHECKING.store(false, Ordering::SeqCst);
        match result {
            Ok(Some(version)) => {
                let is_news = available().as_deref() != Some(version.as_str());
                if is_news || manual {
                    overlay.say(&if cfg!(target_os = "macos") {
                        format!("Mew {version} is out. Get it from Settings.")
                    } else {
                        format!("Mew {version} is ready. I'll switch to it next time I start.")
                    });
                }
                if is_news {
                    *AVAILABLE.lock().unwrap() = Some(version);
                    let _ = pushes.send(json!({ "type": "settings" }));
                    system::wake();
                }
            }
            Ok(None) if manual => overlay.say("You have the newest Mew. Purr."),
            Ok(None) => {}
            Err(e) => {
                log::warn!("update check failed: {e:#}");
                if manual {
                    overlay.huh("I couldn't check for updates right now.");
                }
            }
        }
    });
}

/// Windows restarts into the downloaded update and doesn't return; macOS opens the download page.
pub fn install() -> Result<()> {
    if cfg!(target_os = "macos") {
        system::open(&format!("{FEED}/releases/latest"));
        return Ok(());
    }
    let manager = manager()?;
    let update = manager.get_update_pending_restart().context("no update is waiting")?;
    manager.apply_updates_and_restart(&update)?;
    Ok(())
}

fn manager() -> Result<UpdateManager> {
    Ok(UpdateManager::new(GithubSource::new(FEED, None, false), None, None)?)
}

/// The version now ready to apply, downloading it unless that's already done.
fn download_update(overlay: &Overlay, manual: bool) -> Result<Option<String>> {
    let manager = manager()?;
    let UpdateCheck::UpdateAvailable(info) = manager.check_for_updates()? else { return Ok(None) };
    let version = info.TargetFullRelease.Version.clone();
    if available().as_deref() != Some(version.as_str()) {
        if manual {
            overlay.say(&format!("Found Mew {version}. Downloading it now."));
        }
        manager.download_updates(&info, None)?;
    }
    Ok(Some(version))
}

/// The latest release's version, if it's newer than this build.
fn newest_release() -> Result<Option<String>> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(REQUEST_TIMEOUT)).build().into();
    let release: Value = agent
        .get(LATEST_RELEASE_API)
        .header("User-Agent", concat!("Mew/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_json()?;
    let tag = release["tag_name"].as_str().context("no tag in the release")?;
    Ok(is_newer(tag, env!("CARGO_PKG_VERSION")).then(|| tag.trim_start_matches('v').to_string()))
}

/// Compares `v1.2.3`-style versions; anything else (such as a pre-release) never counts as newer.
fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |version: &str| -> Option<Vec<u64>> {
        version.trim_start_matches('v').split('.').map(|part| part.parse().ok()).collect()
    };
    matches!((parse(tag), parse(current)), (Some(tag), Some(current)) if tag > current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_as_numbers() {
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(is_newer("v0.4.1", "0.4.0"));
        assert!(!is_newer("v0.4.0", "0.4.0"));
        assert!(!is_newer("v0.3.9", "0.4.0"));
        assert!(!is_newer("v0.5.0-beta", "0.4.0"));
        assert!(!is_newer("latest", "0.4.0"));
    }
}
