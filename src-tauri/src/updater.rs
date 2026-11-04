//! Daily check for a newer release. It sends one anonymous GET to GitHub and nothing else;
//! installing stays in the user's hands through the visible install command.

use crate::lifecycle;
use gapir_core::version::is_newer;
use serde::Deserialize;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const LATEST_RELEASE: &str = "https://api.github.com/repos/SalikhovID/gapir/releases/latest";
const INSTALL_COMMAND: &str =
    "curl -fsSL https://raw.githubusercontent.com/SalikhovID/gapir/main/install.sh | bash";
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

fn latest_tag() -> Option<String> {
    let body = ureq::get(LATEST_RELEASE)
        .header("User-Agent", concat!("Gapir/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let release: Release = serde_json::from_str(&body).ok()?;
    Some(release.tag_name)
}

pub fn spawn(app: AppHandle, enabled: impl Fn() -> bool + Send + 'static) {
    std::thread::Builder::new()
        .name("updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                if enabled() {
                    if let Some(tag) = latest_tag() {
                        if is_newer(env!("CARGO_PKG_VERSION"), &tag) {
                            log::info!("update available: {tag}");
                            lifecycle::set_update_available(&app, &tag);
                        }
                    }
                }
                std::thread::sleep(CHECK_INTERVAL);
            }
        })
        .expect("failed to start the update thread");
}

/// Runs the install command in a visible Terminal window. The app is replaced by the script.
pub fn run_update(app: &AppHandle) {
    let result = app
        .path()
        .app_cache_dir()
        .map_err(std::io::Error::other)
        .and_then(|dir| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::create_dir_all(&dir)?;
            let script = dir.join("gapir-update.command");
            std::fs::write(&script, format!("#!/bin/bash\n{INSTALL_COMMAND}\n"))?;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
            std::process::Command::new("/usr/bin/open")
                .arg(&script)
                .status()?;
            Ok(())
        });
    if let Err(error) = result {
        log::error!("update could not start: {error}");
    }
}
