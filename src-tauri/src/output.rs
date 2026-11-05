//! Types into the focused app with synthetic Unicode key events. The clipboard stays untouched,
//! which matters here: text arrives piece by piece and the user may copy something meanwhile.

use crate::hotkeys::Keys;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFEqual, CFRetained, CFString, CFType};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventType,
};
use std::ptr::{null, NonNull};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `CGEventKeyboardSetUnicodeString` handles at most this many UTF-16 units per event.
const MAX_UNITS: usize = 20;
const BACKSPACE_KEYCODE: u16 = 51;
const RETURN_KEYCODE: u16 = 36;
/// Apps where Return sends a prompt: terminals that run coding agents, and Claude.
// shortcut: fixed list, make it a setting if users ask for other apps.
const SEND_APPS: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "com.github.wez.wezterm",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "so.stencil.tern",
    "com.anthropic.claudefordesktop",
];

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

struct Target {
    pid: i32,
    window: Option<CFRetained<CFType>>,
}

pub struct MacOutput {
    keys: Arc<Keys>,
    target: Option<Target>,
}

fn frontmost_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

fn bundle_id(pid: i32) -> Option<String> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?
        .bundleIdentifier()
        .map(|id| id.to_string())
}

fn focused_window(pid: i32) -> Option<CFRetained<CFType>> {
    let attribute = CFString::from_static_str("AXFocusedWindow");
    let mut value: *const CFType = null();
    // SAFETY: `value` is a valid out-pointer; on success it holds a +1 reference we take over.
    unsafe {
        let app = AXUIElement::new_application(pid);
        let status = app.copy_attribute_value(&attribute, NonNull::from(&mut value));
        if status != AXError::Success {
            return None;
        }
        NonNull::new(value.cast_mut()).map(|pointer| CFRetained::from_raw(pointer))
    }
}

/// Splits UTF-16 text into event-sized pieces without tearing a surrogate pair.
fn split_units(units: &[u16]) -> Vec<&[u16]> {
    let mut pieces = Vec::new();
    let mut rest = units;
    while !rest.is_empty() {
        let mut end = rest.len().min(MAX_UNITS);
        if end < rest.len() && (0xD800..0xDC00).contains(&rest[end - 1]) {
            end -= 1;
        }
        let (piece, tail) = rest.split_at(end);
        pieces.push(piece);
        rest = tail;
    }
    pieces
}

/// Posted after the session-level tap, so the listener does not see our own typing as the user's.
fn post(event: &CGEvent) {
    CGEvent::post(CGEventTapLocation::AnnotatedSessionEventTap, Some(event));
}

fn key_event(
    source: Option<&CGEventSource>,
    keycode: u16,
    down: bool,
) -> Option<CFRetained<CGEvent>> {
    let event = CGEvent::new_keyboard_event(source, keycode, down)?;
    // An empty flag set keeps a physically held fn or Cmd from changing the meaning of the event.
    CGEvent::set_flags(Some(&event), CGEventFlags(0));
    Some(event)
}

impl MacOutput {
    pub fn new(keys: Arc<Keys>) -> Self {
        Self { keys, target: None }
    }

    pub fn capture_target(&mut self) {
        self.target = frontmost_pid().map(|pid| Target {
            pid,
            window: focused_window(pid),
        });
    }

    pub fn is_target_focused(&self) -> bool {
        let Some(target) = &self.target else {
            return true;
        };
        if frontmost_pid() != Some(target.pid) {
            return false;
        }
        match (&target.window, focused_window(target.pid)) {
            (Some(wanted), Some(current)) => CFEqual(Some(wanted), Some(&current)),
            // Apps without window accessibility fall back to the process check above.
            _ => true,
        }
    }

    pub fn secure_input(&self) -> bool {
        // SAFETY: plain query without arguments.
        unsafe { IsSecureEventInputEnabled() }
    }

    pub fn type_text(&mut self, text: &str) {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
        let units: Vec<u16> = text.encode_utf16().collect();
        for piece in split_units(&units) {
            for down in [true, false] {
                if let Some(event) = key_event(source.as_deref(), 0, down) {
                    // SAFETY: the pointer and length describe `piece`, which outlives the call.
                    unsafe {
                        CGEvent::keyboard_set_unicode_string(
                            Some(&event),
                            piece.len() as _,
                            piece.as_ptr(),
                        );
                    }
                    post(&event);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn backspace(&mut self, count: usize) {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
        for _ in 0..count {
            for down in [true, false] {
                if let Some(event) = key_event(source.as_deref(), BACKSPACE_KEYCODE, down) {
                    post(&event);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Presses Return if the target is one of [`SEND_APPS`] and still has focus.
    pub fn submit(&mut self) {
        let Some(target) = &self.target else {
            return;
        };
        let allowed = bundle_id(target.pid).is_some_and(|id| SEND_APPS.contains(&id.as_str()));
        if !allowed || !self.is_target_focused() || self.secure_input() {
            return;
        }
        // A pause after the typed text, so terminal apps do not read Return as part of a paste.
        std::thread::sleep(Duration::from_millis(150));
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
        for down in [true, false] {
            if let Some(event) = key_event(source.as_deref(), RETURN_KEYCODE, down) {
                post(&event);
            }
        }
    }

    pub fn user_interacted_since(&self, since: Instant) -> bool {
        if self.keys.user_typed_since(since) {
            return true;
        }
        let elapsed = since.elapsed().as_secs_f64();
        [CGEventType::LeftMouseDown, CGEventType::RightMouseDown]
            .into_iter()
            .any(|kind| {
                CGEventSource::seconds_since_last_event_type(
                    CGEventSourceStateID::HIDSystemState,
                    kind,
                ) < elapsed
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_stays_whole() {
        let units: Vec<u16> = "salom".encode_utf16().collect();
        assert_eq!(split_units(&units), vec![units.as_slice()]);
    }

    #[test]
    fn long_text_is_split_without_losing_anything() {
        let units: Vec<u16> = "a".repeat(45).encode_utf16().collect();
        let pieces = split_units(&units);
        assert_eq!(pieces.len(), 3);
        assert!(pieces.iter().all(|p| p.len() <= MAX_UNITS));
        assert_eq!(pieces.concat(), units);
    }

    #[test]
    fn surrogate_pairs_are_never_torn() {
        let text = format!("{}👍tail", "a".repeat(19));
        let units: Vec<u16> = text.encode_utf16().collect();
        let pieces = split_units(&units);
        assert_eq!(pieces[0].len(), 19);
        assert_eq!(String::from_utf16(pieces[1]).unwrap(), "👍tail");
    }

    #[test]
    fn empty_text_has_no_pieces() {
        assert!(split_units(&[]).is_empty());
    }
}
