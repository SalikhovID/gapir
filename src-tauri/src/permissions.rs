//! System permission checks and shortcuts into System Settings.

use block2::RcBlock;
use handy_recorder::Permission;
use objc2::runtime::Bool;
use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
use objc2_core_graphics::CGRequestPostEventAccess;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// The answer given to the prompt. A running process may keep reporting the old system status.
static MIC_ALLOWED: AtomicBool = AtomicBool::new(false);

pub fn mic_status() -> &'static str {
    if MIC_ALLOWED.load(Ordering::SeqCst) {
        return "granted";
    }
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

/// Shows the system prompt. The device stays closed: opening a stream only to trigger the prompt
/// and closing it right away can leave it stuck, which then blocks the next recording.
pub fn request_mic() {
    // SAFETY: AVFoundation provides this constant for the whole life of the process.
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return;
    };
    let answer = RcBlock::new(|granted: Bool| {
        log::info!("microphone permission answered: {}", granted.as_bool());
        MIC_ALLOWED.store(granted.as_bool(), Ordering::SeqCst);
    });
    // SAFETY: `audio` is a valid media type, and the callee copies the block.
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &answer) };
}

pub fn open_pane(pane: &str) {
    let url = match pane {
        "mic" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        _ => return,
    };
    if let Err(error) = Command::new("/usr/bin/open").arg(url).status() {
        log::warn!("could not open System Settings: {error}");
    }
}
