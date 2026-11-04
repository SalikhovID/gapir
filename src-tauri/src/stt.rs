//! Speech-to-text worker. One thread owns the model and processes chunks in order, so audio
//! simply waits in the queue while the model loads. An idle model is released to free memory.

use gapir_core::engine::Input;
use gapir_core::model_store::{self, ModelSpec};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use transcribe_cpp::{CancelToken, Model, RunOptions, Session, TimestampKind};

pub const MODEL: ModelSpec = ModelSpec {
    url: "https://huggingface.co/nizom333/rubaistt-v2-medium-gguf/resolve/main/rubaistt-v2-medium-Q8_0.gguf",
    file_name: "rubaistt-v2-medium-Q8_0.gguf",
    size: 831_538_240,
    sha256: "2b7a482a4272699a7f54f8c85b48ea1adaefe6a93561a39f923c01a77104d333",
};

const IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

enum Job {
    Prepare,
    Transcribe { session: u64, audio: Vec<f32> },
}

pub struct Stt {
    jobs: Sender<Job>,
    models_dir: PathBuf,
    last_session: u64,
    /// Sessions up to this number were cancelled; their queued jobs are skipped.
    cancelled_through: Arc<AtomicU64>,
    running: Arc<Mutex<Option<CancelToken>>>,
}

impl Stt {
    /// `on_broken` runs when the model file cannot be loaded.
    pub fn new(
        models_dir: PathBuf,
        results: Sender<Input>,
        on_broken: impl Fn() + Send + 'static,
    ) -> Self {
        let (jobs, receiver) = mpsc::channel();
        let cancelled_through = Arc::new(AtomicU64::new(0));
        let running = Arc::new(Mutex::new(None));
        let worker = Worker {
            path: model_store::model_path(&models_dir, &MODEL),
            loaded: None,
            results,
            cancelled_through: cancelled_through.clone(),
            running: running.clone(),
            on_broken: Box::new(on_broken),
        };
        std::thread::Builder::new()
            .name("stt".into())
            .spawn(move || worker.run(receiver))
            .expect("failed to start the transcription thread");
        Self {
            jobs,
            models_dir,
            last_session: 0,
            cancelled_through,
            running,
        }
    }

    pub fn prepare(&mut self) -> bool {
        if !model_store::is_present(&self.models_dir, &MODEL) {
            return false;
        }
        let _ = self.jobs.send(Job::Prepare);
        true
    }

    pub fn transcribe(&mut self, session: u64, audio: Vec<f32>) {
        self.last_session = session;
        let _ = self.jobs.send(Job::Transcribe { session, audio });
    }

    pub fn clear(&mut self) {
        self.cancelled_through
            .store(self.last_session, Ordering::SeqCst);
        if let Some(token) = self.running.lock().expect("cancel token lock").as_ref() {
            token.cancel();
        }
    }
}

struct Loaded {
    session: Session,
    _model: Model,
}

struct Worker {
    path: PathBuf,
    loaded: Option<Loaded>,
    results: Sender<Input>,
    cancelled_through: Arc<AtomicU64>,
    running: Arc<Mutex<Option<CancelToken>>>,
    on_broken: Box<dyn Fn() + Send>,
}

impl Worker {
    fn run(mut self, jobs: mpsc::Receiver<Job>) {
        loop {
            match jobs.recv_timeout(IDLE_UNLOAD) {
                Ok(Job::Prepare) => {
                    self.ensure_loaded();
                }
                Ok(Job::Transcribe { session, audio }) => {
                    if session > self.cancelled_through.load(Ordering::SeqCst) {
                        self.transcribe(session, &audio);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.loaded.take().is_some() {
                        log::info!("model released after being idle");
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn ensure_loaded(&mut self) -> bool {
        if self.loaded.is_some() {
            return true;
        }
        let started = Instant::now();
        let loaded = Model::load(&self.path).and_then(|model| {
            model.session().map(|session| Loaded {
                session,
                _model: model,
            })
        });
        match loaded {
            Ok(loaded) => {
                log::info!("model loaded in {} ms", started.elapsed().as_millis());
                self.loaded = Some(loaded);
                true
            }
            Err(error) => {
                log::error!("model failed to load: {error}");
                (self.on_broken)();
                false
            }
        }
    }

    fn transcribe(&mut self, session: u64, audio: &[f32]) {
        let text = self
            .ensure_loaded()
            .then(|| self.run_with_retry(session, audio));
        let input = match text.flatten() {
            Some(text) => Input::Transcribed { session, text },
            None => Input::TranscribeFailed { session },
        };
        let _ = self.results.send(input);
    }

    /// A failed chunk gets one more try: losing a sentence is worse than a short delay.
    fn run_with_retry(&mut self, session: u64, audio: &[f32]) -> Option<String> {
        let options = RunOptions {
            language: Some("uz".to_string()),
            timestamps: TimestampKind::None,
            ..RunOptions::default()
        };
        let loaded = self.loaded.as_mut()?;
        let token = CancelToken::new();
        loaded.session.set_cancel_token(&token);
        *self.running.lock().expect("cancel token lock") = Some(token);

        let started = Instant::now();
        let mut result = loaded.session.run(audio, &options);
        let aborted = loaded.session.was_aborted();
        if result.is_err() && !aborted && session > self.cancelled_through.load(Ordering::SeqCst) {
            log::warn!("transcription failed, retrying");
            result = loaded.session.run(audio, &options);
        }
        loaded.session.clear_cancel_token();
        *self.running.lock().expect("cancel token lock") = None;

        match result {
            Ok(transcript) => {
                log::info!(
                    "chunk of {:.1} s transcribed in {} ms",
                    audio.len() as f32 / 16_000.0,
                    started.elapsed().as_millis()
                );
                Some(transcript.text)
            }
            Err(error) => {
                log::error!("transcription failed: {error}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a real model: `GAPIR_MODEL=<gguf> GAPIR_WAV=<16 kHz wav> cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn transcribes_real_speech() {
        let model = Model::load(std::env::var("GAPIR_MODEL").unwrap()).unwrap();
        let mut session = model.session().unwrap();
        let bytes = std::fs::read(std::env::var("GAPIR_WAV").unwrap()).unwrap();
        let samples: Vec<f32> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair) as f32 / 32768.0)
            .collect();
        let options = RunOptions {
            language: Some("uz".to_string()),
            timestamps: TimestampKind::None,
            ..RunOptions::default()
        };
        let started = Instant::now();
        let transcript = session.run(&samples, &options).unwrap();
        println!("{} ms: {}", started.elapsed().as_millis(), transcript.text);
        assert!(!transcript.text.trim().is_empty());
    }
}
