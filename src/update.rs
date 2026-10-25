//! Background updates via Velopack. A downloaded update is applied the next time Mew starts.

use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateManager};

use crate::overlay::Overlay;

/// GitHub repository whose Releases hold the Velopack packages; `None` until the project is published.
pub const FEED: Option<&str> = None;

/// `manual` checks (from the tray) always report back; automatic ones only speak up when there's news.
pub fn check(overlay: &Overlay, manual: bool) {
    let overlay = overlay.clone();
    std::thread::spawn(move || {
        let Some(feed) = FEED else {
            if manual {
                overlay.say("Updates aren't switched on for this build yet.");
            }
            return;
        };
        let manager = match UpdateManager::new(GithubSource::new(feed, None, false), None, None) {
            Ok(m) => m,
            Err(e) => {
                log::info!("updates unavailable: {e}");
                if manual {
                    overlay.say("Updates work once Mew is installed.");
                }
                return;
            }
        };
        match manager.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(info)) => {
                let version = info.TargetFullRelease.Version.clone();
                match manager.download_updates(&info, None) {
                    Ok(()) => overlay.say(&format!("Mew {version} is ready. I'll switch to it next time I start.")),
                    Err(e) => log::warn!("update download failed: {e}"),
                }
            }
            Ok(_) if manual => overlay.say("You have the newest Mew. Purr."),
            Ok(_) => {}
            Err(e) => {
                log::warn!("update check failed: {e}");
                if manual {
                    overlay.huh("I couldn't check for updates right now.");
                }
            }
        }
    });
}
