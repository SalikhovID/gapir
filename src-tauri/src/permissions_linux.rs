//! Linux asks desktop apps for no microphone permission. Hotkeys and typing go through the kernel
//! input devices instead, so the "Accessibility" step is access to `/dev/input` and `/dev/uinput`.

use std::fs::{File, OpenOptions};

/// Run once in a terminal, then log out and back in for the new group to apply.
pub const SETUP_COMMAND: &str = "echo 'KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\", OPTIONS+=\"static_node=uinput\"' | sudo tee /etc/udev/rules.d/60-gapir.rules && sudo modprobe uinput && sudo udevadm control --reload && sudo udevadm trigger && sudo usermod -aG input $USER";

/// PulseAudio and PipeWire let every desktop app record.
pub fn mic_status() -> &'static str {
    "granted"
}

/// Reading a keyboard hears the hotkeys; writing uinput types the text.
pub fn accessibility() -> bool {
    let keyboards = std::fs::read_dir("/dev/input")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("event"))
        .any(|entry| File::open(entry.path()).is_ok());
    keyboards && OpenOptions::new().write(true).open("/dev/uinput").is_ok()
}

pub fn request_mic() {}

pub fn open_pane(_pane: &str) {}
