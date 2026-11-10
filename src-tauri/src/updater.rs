//! Checks GitHub for a newer release every few minutes and installs it in the background while
//! Gapir is idle. The check is one anonymous GET; the install runs the official installer.

use crate::{Shared, lifecycle};
use gapir_core::version::is_newer;
use serde::Deserialize;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const LATEST_RELEASE: &str = "https://api.github.com/repos/SalikhovID/gapir/releases/latest";
/// `GAPIR_UPDATE=1` makes the script reopen the app hidden, without the main window.
#[cfg(target_os = "macos")]
const INSTALL_COMMAND: &str = "curl -fsSL https://raw.githubusercontent.com/SalikhovID/gapir/main/install.sh | GAPIR_UPDATE=1 bash";
/// Downloads the installer, checks its hash and runs it silently: it closes Gapir and starts
/// the new version hidden (`/R /ARGS --hidden`).
#[cfg(windows)]
const INSTALL_COMMAND: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$url = 'https://github.com/SalikhovID/gapir/releases/latest/download/Gapir-windows-x64-setup.exe'
$file = Join-Path $env:TEMP 'Gapir-windows-x64-setup.exe'
Invoke-WebRequest $url -OutFile $file -UseBasicParsing
Invoke-WebRequest "$url.sha256" -OutFile "$file.sha256" -UseBasicParsing
$expected = (Get-Content "$file.sha256" -Raw).Split(' ')[0]
if ((Get-FileHash $file -Algorithm SHA256).Hash -ne $expected) { throw 'downloaded installer is damaged' }
Start-Process $file '/S /R /ARGS --hidden'
"#;
const FIRST_CHECK: Duration = Duration::from_secs(60);
/// 12 requests an hour, well under GitHub's anonymous limit of 60.
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

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

pub fn spawn(app: AppHandle, shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            // One attempt per release, so a failing install does not repeat every few minutes.
            let mut attempted: Option<String> = None;
            loop {
                if shared.settings().update_check
                    && let Some(tag) = latest_tag()
                    && is_newer(env!("CARGO_PKG_VERSION"), &tag)
                {
                    lifecycle::set_update_available(&app, &tag);
                    let idle = !shared.busy.load(Ordering::Relaxed)
                        && !shared.downloading.load(Ordering::Relaxed);
                    if idle && attempted.as_deref() != Some(tag.as_str()) {
                        log::info!("installing update {tag}");
                        run_update(&app);
                        attempted = Some(tag);
                    }
                }
                std::thread::sleep(CHECK_INTERVAL);
            }
        })
        .expect("failed to start the update thread");
}

/// Starts the installer in the background; its output goes to `gapir-update.log` in the cache
/// directory. The installer closes this app and starts the new version.
pub fn run_update(app: &AppHandle) {
    let result = app
        .path()
        .app_cache_dir()
        .map_err(std::io::Error::other)
        .and_then(|dir| {
            std::fs::create_dir_all(&dir)?;
            let log = std::fs::File::create(dir.join("gapir-update.log"))?;
            install_command(&dir)?
                .stdout(log.try_clone()?)
                .stderr(log)
                .spawn()?;
            Ok(())
        });
    if let Err(error) = result {
        log::error!("update could not start: {error}");
    }
}

#[cfg(target_os = "macos")]
fn install_command(_dir: &std::path::Path) -> std::io::Result<Command> {
    let mut command = Command::new("/bin/bash");
    command.args(["-c", INSTALL_COMMAND]);
    Ok(command)
}

#[cfg(windows)]
fn install_command(dir: &std::path::Path) -> std::io::Result<Command> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // A file, not -Command: PowerShell 5 mangles quotes passed on the command line.
    let script = dir.join("gapir-update.ps1");
    std::fs::write(&script, INSTALL_COMMAND)?;
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .creation_flags(CREATE_NO_WINDOW);
    Ok(command)
}
