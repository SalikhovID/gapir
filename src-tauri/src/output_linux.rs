//! Pastes into the focused window: the text goes to the clipboard, then Shift+Insert comes from a
//! uinput keyboard. Only the kernel reaches every app on both X11 and Wayland, and its keys know
//! no Unicode, so letters like "ʻ" have to travel through the clipboard.

use crate::hotkeys::Keys;
use arboard::{Clipboard, LinuxClipboardKind, SetExtLinux};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Time for the target app to read the clipboard before the next piece replaces it.
const PASTE_TIME: Duration = Duration::from_millis(200);
/// Between key events, so no compositor merges or drops them.
const KEY_GAP: Duration = Duration::from_millis(2);
/// Our keys come back through the hotkey listener a moment after they are sent.
const OWN_INPUT_SLACK: Duration = Duration::from_millis(100);
const PASTE: [(KeyCode, i32); 4] = [
    (KeyCode::KEY_LEFTSHIFT, 1),
    (KeyCode::KEY_INSERT, 1),
    (KeyCode::KEY_INSERT, 0),
    (KeyCode::KEY_LEFTSHIFT, 0),
];

pub struct SystemOutput {
    keys: Arc<Keys>,
    keyboard: Option<VirtualDevice>,
}

/// One clipboard for the whole app: on X11 the text is gone once its owner is dropped.
fn with_clipboard(set: impl FnOnce(&mut Clipboard) -> Result<(), arboard::Error>) -> bool {
    static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);
    let mut slot = CLIPBOARD.lock().expect("clipboard lock");
    if slot.is_none() {
        *slot = Clipboard::new()
            .inspect_err(|error| log::warn!("clipboard: {error}"))
            .ok();
    }
    let Some(clipboard) = slot.as_mut() else {
        return false;
    };
    set(clipboard)
        .inspect_err(|error| log::warn!("clipboard: {error}"))
        .is_ok()
}

fn virtual_keyboard() -> std::io::Result<VirtualDevice> {
    // Every regular key, so udev and the compositor take it for a full keyboard.
    let keys: AttributeSet<KeyCode> = (1..=248).map(KeyCode::new).collect();
    let device = VirtualDevice::builder()?
        .name("Gapir keyboard")
        .with_keys(&keys)?
        .build()?;
    // The compositor and the hotkey listener need a moment to pick up a new device.
    std::thread::sleep(Duration::from_millis(500));
    Ok(device)
}

impl SystemOutput {
    pub fn new(keys: Arc<Keys>) -> Self {
        let keyboard = virtual_keyboard()
            .inspect_err(|error| log::error!("virtual keyboard: {error}"))
            .ok();
        Self { keys, keyboard }
    }

    fn send(&mut self, events: &[(KeyCode, i32)]) {
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        self.keys
            .ignore_input_until(Instant::now() + KEY_GAP * events.len() as u32 + OWN_INPUT_SLACK);
        for &(code, value) in events {
            let event = InputEvent::new(EventType::KEY.0, code.0, value);
            if let Err(error) = keyboard.emit(&[event]) {
                log::warn!("virtual keyboard: {error}");
                return;
            }
            std::thread::sleep(KEY_GAP);
        }
    }

    /// Wayland tells no app which window has focus, so the target is not remembered.
    pub fn capture_target(&mut self) {}

    /// Text waits while the hold key is down: held Ctrl would turn the paste into another shortcut.
    pub fn is_target_focused(&self) -> bool {
        !self.keys.hold_key_down()
    }

    pub fn secure_input(&self) -> bool {
        false
    }

    pub fn type_text(&mut self, text: &str) {
        let copied = with_clipboard(|clipboard| {
            clipboard.set_text(text)?;
            // xterm, kitty and Alacritty paste the primary selection on Shift+Insert.
            let _ = clipboard
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text);
            Ok(())
        });
        // A failed copy would paste whatever the clipboard held before.
        if !copied {
            return;
        }
        self.send(&PASTE);
        std::thread::sleep(PASTE_TIME);
    }

    pub fn backspace(&mut self, count: usize) {
        let taps: Vec<_> = (0..count)
            .flat_map(|_| [(KeyCode::KEY_BACKSPACE, 1), (KeyCode::KEY_BACKSPACE, 0)])
            .collect();
        self.send(&taps);
    }

    // shortcut: never presses Return, since Wayland hides which app has focus and terminals
    // cannot be told apart; add an X11 window check if users ask.
    pub fn submit(&mut self) {}

    pub fn user_interacted_since(&self, since: Instant) -> bool {
        self.keys.user_typed_since(since)
    }
}

pub fn set_clipboard(text: &str) {
    with_clipboard(|clipboard| clipboard.set_text(text));
}
