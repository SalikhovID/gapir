//! Windows needs no Accessibility grant, and it never prompts a desktop app for the microphone:
//! only the privacy switches in Settings can block it.

use handy_recorder::Permission;
use std::process::Command;

/// Missing or "Prompt" switches still let desktop apps record, so only an explicit "Deny" counts.
pub fn mic_status() -> &'static str {
    match handy_recorder::permission_status() {
        Permission::Denied => "denied",
        _ => "granted",
    }
}

pub fn accessibility() -> bool {
    true
}

pub fn request_mic() {
    open_pane("mic");
}

pub fn open_pane(pane: &str) {
    if pane != "mic" {
        return;
    }
    if let Err(error) = Command::new("explorer")
        .arg("ms-settings:privacy-microphone")
        .status()
    {
        log::warn!("could not open Settings: {error}");
    }
}
