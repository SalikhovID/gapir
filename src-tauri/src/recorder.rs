//! Microphone capture. The device stays closed outside recording, so macOS shows no indicator
//! and Bluetooth headsets keep their music quality.

use gapir_core::engine::Input;
use gapir_core::machine::Event;
use handy_recorder::{AudioChunk, Recorder, RecorderConfig, Sink};
use std::sync::mpsc::{self, Sender};

/// Marker in the CoreAudio device id of the Mac's own microphone.
const BUILT_IN_MARKER: &str = "BuiltIn";

/// Chooses the device to open. `None` means the system default. With no explicit choice the
/// built-in microphone wins, because Bluetooth input degrades both audio and start-up time.
pub fn pick_device(setting: Option<&str>, device_ids: &[String]) -> Option<String> {
    if let Some(wanted) = setting {
        if device_ids.iter().any(|id| id == wanted) {
            return Some(wanted.to_string());
        }
    }
    device_ids
        .iter()
        .find(|id| id.contains(BUILT_IN_MARKER))
        .cloned()
}

struct AudioSink {
    input: Sender<Input>,
    started: bool,
}

impl Sink for AudioSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        if !self.started {
            self.started = true;
            let _ = self.input.send(Input::MicReady);
        }
        let _ = self
            .input
            .send(Input::Audio(chunk.samples[..chunk.valid_frames].to_vec()));
    }
}

enum Command {
    Start(Option<String>),
    Stop,
}

pub struct Mic {
    commands: Sender<Command>,
}

impl Mic {
    pub fn new(input: Sender<Input>) -> Self {
        let (commands, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("mic".into())
            .spawn(move || {
                let mut active: Option<Recorder<AudioSink>> = None;
                for command in receiver {
                    match command {
                        Command::Start(setting) => {
                            close(active.take());
                            active = open(setting.as_deref(), &input);
                            if active.is_none() {
                                let _ = input.send(Input::Key(Event::DeviceLost));
                            }
                        }
                        Command::Stop => close(active.take()),
                    }
                }
            })
            .expect("failed to start the microphone thread");
        Self { commands }
    }

    pub fn start(&self, setting: Option<String>) {
        let _ = self.commands.send(Command::Start(setting));
    }

    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }
}

fn open(setting: Option<&str>, input: &Sender<Input>) -> Option<Recorder<AudioSink>> {
    let ids: Vec<String> = handy_recorder::list_input_devices()
        .map(|devices| devices.into_iter().map(|d| d.id).collect())
        .unwrap_or_default();
    let config = RecorderConfig {
        device: pick_device(setting, &ids),
        ..RecorderConfig::speech()
    };

    let failures = input.clone();
    let recorder = Recorder::open_with_failure_handler(config, move |error| {
        log::warn!("microphone failed: {error}");
        let _ = failures.send(Input::Key(Event::DeviceLost));
    });
    let recorder = match recorder {
        Ok(recorder) => recorder,
        Err(error) => {
            log::error!("could not open the microphone: {error}");
            return None;
        }
    };
    log::info!("recording from {}", recorder.info().device.name);

    let sink = AudioSink {
        input: input.clone(),
        started: false,
    };
    if let Err(error) = recorder.start(sink) {
        log::error!("could not start recording: {}", error.error);
        return None;
    }
    Some(recorder)
}

fn close(recorder: Option<Recorder<AudioSink>>) {
    if let Some(recorder) = recorder {
        if let Err(error) = recorder.stop() {
            log::debug!("recorder stop: {error}");
        }
        if let Err(error) = recorder.close() {
            log::debug!("recorder close: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        vec![
            "coreaudio:AirPodsUID:input".to_string(),
            "coreaudio:BuiltInMicrophoneDevice".to_string(),
            "coreaudio:usb-mic".to_string(),
        ]
    }

    #[test]
    fn explicit_choice_wins() {
        assert_eq!(
            pick_device(Some("coreaudio:usb-mic"), &ids()),
            Some("coreaudio:usb-mic".to_string())
        );
    }

    #[test]
    fn built_in_microphone_is_preferred_over_bluetooth() {
        assert_eq!(
            pick_device(None, &ids()),
            Some("coreaudio:BuiltInMicrophoneDevice".to_string())
        );
    }

    #[test]
    fn without_a_built_in_microphone_the_system_default_is_used() {
        assert_eq!(pick_device(None, &["coreaudio:usb-mic".to_string()]), None);
    }

    #[test]
    fn a_vanished_device_falls_back_to_automatic_choice() {
        assert_eq!(
            pick_device(Some("coreaudio:gone"), &ids()),
            Some("coreaudio:BuiltInMicrophoneDevice".to_string())
        );
    }
}
