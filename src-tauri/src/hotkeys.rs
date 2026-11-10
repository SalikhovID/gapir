//! Global keyboard handling: turns raw key events into engine events. One listener sees every
//! key, which also gives the "did the user touch the keyboard" signal for safe text removal.

use gapir_core::engine::Input;
use gapir_core::machine::Event;
use gapir_core::settings::Settings;
use handy_keys::{BlockingHotkeys, Hotkey, Key, KeyEvent, KeyboardListener, Modifiers};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct KeyConfig {
    pub fn_enabled: bool,
    pub toggle: Hotkey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Machine(Event),
    UserInput,
}

/// The hold-to-talk key. PC keyboards handle `fn` in firmware, so Windows never sees it.
const HOLD_KEY: Modifiers = if cfg!(windows) {
    Modifiers::CTRL_RIGHT
} else {
    Modifiers::FN
};

/// Maps the raw event stream to signals. Stateful because `fn` chords and auto-repeat
/// can only be told apart by what is currently held.
#[derive(Debug, Default)]
pub struct Classifier {
    fn_held: bool,
    toggle_down: bool,
}

impl Classifier {
    pub fn classify(&mut self, event: &KeyEvent, config: &KeyConfig) -> Vec<Signal> {
        if event.changed_modifier == Some(HOLD_KEY) {
            if !config.fn_enabled {
                return vec![];
            }
            self.fn_held = event.is_key_down;
            let signal = if event.is_key_down {
                Event::FnDown
            } else {
                Event::FnUp
            };
            return vec![Signal::Machine(signal)];
        }

        let Some(key) = event.key else {
            return vec![];
        };
        let is_toggle_key = Some(key) == config.toggle.key;

        if !event.is_key_down {
            if is_toggle_key {
                self.toggle_down = false;
            }
            return vec![];
        }
        if is_toggle_key && config.toggle.modifiers.matches(event.modifiers) {
            let repeated = std::mem::replace(&mut self.toggle_down, true);
            return if repeated {
                vec![]
            } else {
                vec![Signal::Machine(Event::ToggleHotkey)]
            };
        }
        if key == Key::Escape {
            return vec![Signal::Machine(Event::Esc)];
        }
        if self.fn_held {
            return vec![Signal::Machine(Event::FnChord), Signal::UserInput];
        }
        vec![Signal::UserInput]
    }
}

fn esc_hotkey() -> Hotkey {
    Hotkey::new(Modifiers::empty(), Key::Escape).expect("Esc alone is a valid hotkey")
}

/// Parses the hotkey from settings, falling back to the default if it is broken.
pub fn parse_toggle(text: &str) -> Hotkey {
    text.parse().unwrap_or_else(|_| {
        log::warn!("invalid toggle hotkey, using the default");
        Settings::default()
            .toggle_hotkey
            .parse()
            .expect("default hotkey is valid")
    })
}

/// Shared control surface of the keyboard listener.
pub struct Keys {
    blocking: BlockingHotkeys,
    config: Mutex<KeyConfig>,
    capture_esc: AtomicBool,
    paused: AtomicBool,
    last_input: Mutex<Option<Instant>>,
}

impl Keys {
    pub fn new(settings: &Settings) -> Arc<Self> {
        let keys = Arc::new(Self {
            blocking: Arc::new(Mutex::new(HashSet::new())),
            config: Mutex::new(KeyConfig {
                fn_enabled: settings.fn_enabled,
                toggle: parse_toggle(&settings.toggle_hotkey),
            }),
            capture_esc: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            last_input: Mutex::new(None),
        });
        keys.refresh_blocking();
        keys
    }

    fn refresh_blocking(&self) {
        let config = *self.config.lock().expect("key config lock");
        let mut set = self.blocking.lock().expect("blocking set lock");
        set.clear();
        if self.paused.load(Ordering::SeqCst) {
            return;
        }
        // `fn` is only observed. Swallowing it hides the press from macOS, and the layout
        // switcher then reopens on every press instead of animating between layouts.
        set.insert(config.toggle);
        if self.capture_esc.load(Ordering::SeqCst) {
            set.insert(esc_hotkey());
        }
    }

    pub fn apply_settings(&self, settings: &Settings) {
        *self.config.lock().expect("key config lock") = KeyConfig {
            fn_enabled: settings.fn_enabled,
            toggle: parse_toggle(&settings.toggle_hotkey),
        };
        self.refresh_blocking();
    }

    /// Esc belongs to the frontmost app except while a session can be cancelled.
    pub fn capture_esc(&self, on: bool) {
        self.capture_esc.store(on, Ordering::SeqCst);
        self.refresh_blocking();
    }

    /// Lets every key through while the settings window records a new hotkey.
    pub fn pause(&self, on: bool) {
        self.paused.store(on, Ordering::SeqCst);
        self.refresh_blocking();
    }

    pub fn user_typed_since(&self, since: Instant) -> bool {
        self.last_input
            .lock()
            .expect("last input lock")
            .is_some_and(|last| last >= since)
    }

    fn handle(&self, classifier: &mut Classifier, event: &KeyEvent, input: &Sender<Input>) {
        if self.paused.load(Ordering::SeqCst) {
            return;
        }
        let config = *self.config.lock().expect("key config lock");
        for signal in classifier.classify(event, &config) {
            match signal {
                Signal::Machine(event) => {
                    log::info!("key: {event:?}");
                    let _ = input.send(Input::Key(event));
                }
                Signal::UserInput => {
                    *self.last_input.lock().expect("last input lock") = Some(Instant::now());
                }
            }
        }
    }
}

/// Runs the listener for the life of the app. It starts once Accessibility is granted, and
/// `on_access` reports changes so the menu can warn when the permission is taken away.
pub fn spawn(keys: Arc<Keys>, input: Sender<Input>, on_access: impl Fn(bool) + Send + 'static) {
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || {
            let mut had_access = None;
            loop {
                let access = crate::permissions::accessibility();
                if had_access != Some(access) {
                    on_access(access);
                    had_access = Some(access);
                }
                if !access {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                let listener = match KeyboardListener::new_with_blocking(keys.blocking.clone()) {
                    Ok(listener) => listener,
                    Err(error) => {
                        log::error!("keyboard listener failed to start: {error}");
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                };
                log::info!("keyboard listener started");
                let mut classifier = Classifier::default();
                loop {
                    match listener.recv_timeout(Duration::from_secs(2)) {
                        Ok(event) => keys.handle(&mut classifier, &event, &input),
                        Err(handy_keys::Error::Timeout) => {
                            if !crate::permissions::accessibility() {
                                break;
                            }
                        }
                        Err(error) => {
                            log::error!("keyboard listener stopped: {error}");
                            break;
                        }
                    }
                }
            }
        })
        .expect("failed to start the hotkey thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> KeyConfig {
        KeyConfig {
            fn_enabled: true,
            toggle: "Cmd+Shift+G".parse().unwrap(),
        }
    }

    fn fn_key(down: bool) -> KeyEvent {
        KeyEvent {
            modifiers: if down { HOLD_KEY } else { Modifiers::empty() },
            key: None,
            is_key_down: down,
            changed_modifier: Some(HOLD_KEY),
        }
    }

    fn key(modifiers: Modifiers, key: Key, down: bool) -> KeyEvent {
        KeyEvent {
            modifiers,
            key: Some(key),
            is_key_down: down,
            changed_modifier: None,
        }
    }

    fn cmd_shift() -> Modifiers {
        Modifiers::CMD_LEFT | Modifiers::SHIFT_LEFT
    }

    #[test]
    fn fn_press_and_release_map_to_machine_events() {
        let mut c = Classifier::default();
        assert_eq!(
            c.classify(&fn_key(true), &config()),
            vec![Signal::Machine(Event::FnDown)]
        );
        assert_eq!(
            c.classify(&fn_key(false), &config()),
            vec![Signal::Machine(Event::FnUp)]
        );
    }

    #[test]
    fn disabled_fn_is_ignored() {
        let mut c = Classifier::default();
        let cfg = KeyConfig {
            fn_enabled: false,
            ..config()
        };
        assert!(c.classify(&fn_key(true), &cfg).is_empty());
    }

    #[test]
    fn toggle_fires_once_while_the_key_auto_repeats() {
        let mut c = Classifier::default();
        let down = key(cmd_shift(), Key::G, true);
        assert_eq!(
            c.classify(&down, &config()),
            vec![Signal::Machine(Event::ToggleHotkey)]
        );
        assert!(c.classify(&down, &config()).is_empty());
        assert!(
            c.classify(&key(cmd_shift(), Key::G, false), &config())
                .is_empty()
        );
        assert_eq!(
            c.classify(&down, &config()),
            vec![Signal::Machine(Event::ToggleHotkey)]
        );
    }

    #[test]
    fn toggle_key_without_modifiers_is_ordinary_typing() {
        let mut c = Classifier::default();
        assert_eq!(
            c.classify(&key(Modifiers::empty(), Key::G, true), &config()),
            vec![Signal::UserInput]
        );
    }

    #[test]
    fn esc_is_a_machine_event_and_not_user_typing() {
        let mut c = Classifier::default();
        assert_eq!(
            c.classify(&key(Modifiers::empty(), Key::Escape, true), &config()),
            vec![Signal::Machine(Event::Esc)]
        );
    }

    #[test]
    fn other_keys_count_as_user_input() {
        let mut c = Classifier::default();
        assert_eq!(
            c.classify(&key(Modifiers::empty(), Key::A, true), &config()),
            vec![Signal::UserInput]
        );
    }

    #[test]
    fn a_key_pressed_with_fn_is_a_chord() {
        let mut c = Classifier::default();
        c.classify(&fn_key(true), &config());
        assert_eq!(
            c.classify(&key(HOLD_KEY, Key::RightArrow, true), &config()),
            vec![Signal::Machine(Event::FnChord), Signal::UserInput]
        );
    }

    #[test]
    fn arrow_keys_reporting_the_fn_flag_are_not_chords_without_fn_held() {
        let mut c = Classifier::default();
        assert_eq!(
            c.classify(&key(Modifiers::FN, Key::RightArrow, true), &config()),
            vec![Signal::UserInput]
        );
    }

    #[test]
    fn plain_modifier_changes_are_ignored() {
        let mut c = Classifier::default();
        let shift = KeyEvent {
            modifiers: Modifiers::SHIFT_LEFT,
            key: None,
            is_key_down: true,
            changed_modifier: Some(Modifiers::SHIFT_LEFT),
        };
        assert!(c.classify(&shift, &config()).is_empty());
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut c = Classifier::default();
        assert!(
            c.classify(&key(Modifiers::empty(), Key::A, false), &config())
                .is_empty()
        );
    }

    fn blocked(keys: &Keys) -> HashSet<Hotkey> {
        keys.blocking.lock().unwrap().clone()
    }

    #[test]
    fn fn_always_reaches_macos_so_its_layout_switcher_keeps_working() {
        let keys = Keys::new(&Settings::default());
        keys.capture_esc(true);
        assert!(
            blocked(&keys)
                .iter()
                .all(|hotkey| !hotkey.modifiers.contains(Modifiers::FN))
        );
    }

    #[test]
    fn toggle_is_always_blocked_and_esc_only_during_a_session() {
        let keys = Keys::new(&Settings::default());
        let toggle: Hotkey = Settings::default().toggle_hotkey.parse().unwrap();
        assert_eq!(blocked(&keys), HashSet::from([toggle]));
        keys.capture_esc(true);
        assert_eq!(blocked(&keys), HashSet::from([toggle, esc_hotkey()]));
        keys.capture_esc(false);
        assert_eq!(blocked(&keys), HashSet::from([toggle]));
    }

    #[test]
    fn broken_hotkey_text_falls_back_to_the_default() {
        let default: Hotkey = Settings::default().toggle_hotkey.parse().unwrap();
        assert_eq!(parse_toggle("not a hotkey"), default);
    }
}
