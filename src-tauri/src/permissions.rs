//! System permission checks and shortcuts into System Settings.

use handy_recorder::Permission;
use objc2_core_graphics::CGRequestPostEventAccess;
use std::process::Command;

pub fn mic_status() -> &'static str {
    match handy_recorder::permission_status() {
        Permission::Granted => "granted",
        Permission::Denied => "denied",
        _ => "unknown",
    }
}

pub fn accessibility() -> bool {
    handy_keys::check_accessibility()
}

/// Adds the app to the Accessibility list and shows the system prompt.
pub fn request_accessibility() {
    CGRequestPostEventAccess();
}

/// Opening the device is what makes macOS show the microphone prompt.
pub fn request_mic() {
    std::thread::spawn(|| {
        use handy_recorder::{CollectingSink, Recorder, RecorderConfig};
        if let Ok(recorder) = Recorder::<CollectingSink>::open(RecorderConfig::speech()) {
            let _ = recorder.close();
        }
    });
}

/// The system action of the Globe key: 0 means "Do Nothing". `None` when never configured.
pub fn fn_usage() -> Option<i64> {
    let output = Command::new("/usr/bin/defaults")
        .args(["read", "com.apple.HIToolbox", "AppleFnUsageType"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

pub fn open_pane(pane: &str) {
    let url = match pane {
        "mic" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        "keyboard" => "x-apple.systempreferences:com.apple.Keyboard-Settings.extension",
        _ => return,
    };
    if let Err(error) = Command::new("/usr/bin/open").arg(url).status() {
        log::warn!("could not open System Settings: {error}");
    }
}
