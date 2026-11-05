//! User settings stored as JSON in the app data directory.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub fn_enabled: bool,
    /// Hotkey in `handy-keys` syntax.
    pub toggle_hotkey: String,
    /// Input device id. `None` prefers the built-in microphone.
    pub mic_device: Option<String>,
    pub autostart: bool,
    pub sounds: bool,
    pub update_check: bool,
    /// Press Return after dictation in terminals and Claude, so the prompt is sent.
    pub auto_send: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fn_enabled: true,
            toggle_hotkey: "Cmd+Shift+G".into(),
            mic_device: None,
            autostart: true,
            sounds: false,
            update_check: true,
            auto_send: false,
        }
    }
}

impl Settings {
    /// Never fails: a missing or damaged file must not stop dictation.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes through a temporary file so a crash cannot leave a half-written settings file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        (dir, path)
    }

    #[test]
    fn missing_file_gives_defaults() {
        let (_dir, path) = path();
        let s = Settings::load(&path);
        assert!(s.fn_enabled);
        assert_eq!(s.toggle_hotkey, "Cmd+Shift+G");
        assert_eq!(s.mic_device, None);
        assert!(s.autostart);
        assert!(!s.sounds);
        assert!(s.update_check);
    }

    #[test]
    fn save_and_load_round_trip() {
        let (_dir, path) = path();
        let mut s = Settings::load(&path);
        s.fn_enabled = false;
        s.toggle_hotkey = "Ctrl+Alt+R".into();
        s.mic_device = Some("usb-mic".into());
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn unknown_fields_are_ignored_and_missing_fields_default() {
        let (_dir, path) = path();
        std::fs::write(&path, r#"{"sounds": true, "from_the_future": 1}"#).unwrap();
        let s = Settings::load(&path);
        assert!(s.sounds);
        assert!(s.fn_enabled);
    }

    #[test]
    fn corrupt_file_gives_defaults_instead_of_crashing() {
        let (_dir, path) = path();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn save_creates_the_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/settings.json");
        Settings::default().save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn save_leaves_no_temporary_file_behind() {
        let (dir, path) = path();
        Settings::default().save(&path).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }
}
