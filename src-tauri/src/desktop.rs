//! Connects the platform-independent engine to the real microphone, model, keyboard and UI.

use crate::hotkeys::Keys;
use crate::output::MacOutput;
use crate::recorder::Mic;
use crate::stt::Stt;
use crate::{lifecycle, overlay, Shared};
use gapir_core::delivery::Output;
use gapir_core::engine::{Cue, Notice, Platform, View};
use std::sync::Arc;
use std::time::Instant;
use tauri::AppHandle;

pub struct Desktop {
    app: AppHandle,
    shared: Arc<Shared>,
    keys: Arc<Keys>,
    output: MacOutput,
    mic: Mic,
    stt: Stt,
}

impl Desktop {
    pub fn new(app: AppHandle, shared: Arc<Shared>, mic: Mic, stt: Stt) -> Self {
        let keys = shared.keys.clone();
        Self {
            app,
            shared,
            output: MacOutput::new(keys.clone()),
            keys,
            mic,
            stt,
        }
    }
}

impl Output for Desktop {
    fn capture_target(&mut self) {
        self.output.capture_target();
    }
    fn is_target_focused(&self) -> bool {
        self.output.is_target_focused()
    }
    fn secure_input(&self) -> bool {
        self.output.secure_input()
    }
    fn type_text(&mut self, text: &str) {
        self.output.type_text(text);
    }
    fn backspace(&mut self, count: usize) {
        self.output.backspace(count);
    }
    fn user_interacted_since(&self, since: Instant) -> bool {
        self.output.user_interacted_since(since)
    }
}

impl Platform for Desktop {
    fn mic_start(&mut self) {
        let device = self.shared.settings().mic_device;
        self.mic.start(device);
    }
    fn mic_stop(&mut self) {
        self.mic.stop();
    }
    fn stt_prepare(&mut self) -> bool {
        self.stt.prepare()
    }
    fn stt_transcribe(&mut self, session: u64, audio: Vec<f32>) {
        self.stt.transcribe(session, audio);
    }
    fn stt_clear(&mut self) {
        self.stt.clear();
    }
    fn show(&mut self, view: View) {
        overlay::show_view(&self.app, view);
        lifecycle::set_tray_view(&self.app, view);
    }
    fn notify(&mut self, notice: Notice) {
        overlay::notify(&self.app, notice);
    }
    fn set_last_text(&mut self, text: &str) {
        *self.shared.last_text.lock().expect("last text lock") = text.to_string();
        lifecycle::set_last_text_available(&self.app);
    }
    fn capture_esc(&mut self, on: bool) {
        self.keys.capture_esc(on);
    }
    fn cue(&mut self, cue: Cue) {
        if self.shared.settings().sounds {
            overlay::play_cue(&self.app, cue);
        }
    }
    fn need_setup(&mut self) {
        lifecycle::show_main(&self.app);
    }
}
