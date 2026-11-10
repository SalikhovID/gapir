//! Runs one dictation session end to end: reacts to keys, audio and transcription results, and
//! drives the platform through [`Platform`]. All timing and ordering rules live here.

use crate::chunker::{Chunker, Config, Vad};
use crate::delivery::{CancelOutcome, Delivery, Output};
use crate::machine::{self, Action, Event, FN_HOLD_DELAY, Message, State};
use std::time::{Duration, Instant};

/// Toggle recording stops itself after this much silence.
pub const SILENCE_LIMIT_MS: usize = 30_000;
pub const MAX_RECORDING: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Hidden,
    /// Key pressed, microphone still opening.
    Starting,
    Recording {
        elapsed_s: u64,
        waiting: bool,
    },
    Transcribing {
        waiting: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    StillTranscribing,
    SilenceStopped,
    LimitReached,
    MicLost,
    NoSpeech,
    TextInMenu,
    RemovalRefused,
    ChunkFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start,
    Stop,
}

/// Everything the engine needs from the operating system.
pub trait Platform: Output {
    fn mic_start(&mut self);
    fn mic_stop(&mut self);
    /// Loads the model if needed. False when the model is not installed.
    fn stt_prepare(&mut self) -> bool;
    fn stt_transcribe(&mut self, session: u64, audio: Vec<f32>);
    /// Drops queued audio and aborts the running transcription.
    fn stt_clear(&mut self);
    fn show(&mut self, view: View);
    fn notify(&mut self, notice: Notice);
    fn set_last_text(&mut self, text: &str);
    fn capture_esc(&mut self, on: bool);
    fn cue(&mut self, cue: Cue);
    /// Opens the setup window.
    fn need_setup(&mut self);
}

#[derive(Debug)]
pub enum Input {
    Key(Event),
    MicReady,
    Audio(Vec<f32>),
    Transcribed { session: u64, text: String },
    TranscribeFailed { session: u64 },
    Tick,
}

pub struct Engine<P: Platform, V: Vad> {
    platform: P,
    make_vad: fn() -> V,
    state: State,
    chunker: Option<Chunker<V>>,
    delivery: Delivery,
    last_session: u64,
    /// Results are accepted only for this session, so cancelled work can never be typed.
    current_session: Option<u64>,
    in_flight: usize,
    started_at: Option<Instant>,
    mic_ready: bool,
    /// The reason recording stopped was already shown, so "no speech" would be noise.
    announced: bool,
    view: View,
}

impl<P: Platform, V: Vad> Engine<P, V> {
    pub fn new(platform: P, make_vad: fn() -> V) -> Self {
        Self {
            platform,
            make_vad,
            state: State::Idle,
            chunker: None,
            delivery: Delivery::new(),
            last_session: 0,
            current_session: None,
            in_flight: 0,
            started_at: None,
            mic_ready: false,
            announced: false,
            view: View::Hidden,
        }
    }

    pub fn platform(&self) -> &P {
        &self.platform
    }

    pub fn platform_mut(&mut self) -> &mut P {
        &mut self.platform
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The moment a held `fn` turns into a recording, so the caller can wake up exactly then.
    pub fn hold_deadline(&self) -> Option<Instant> {
        match self.state {
            State::Armed { since } => Some(since + FN_HOLD_DELAY),
            _ => None,
        }
    }

    pub fn handle(&mut self, input: Input, now: Instant) {
        match input {
            Input::Key(event) => self.apply(event, now),
            Input::MicReady => {
                if self.is_recording() && !self.mic_ready {
                    self.mic_ready = true;
                    self.platform.cue(Cue::Start);
                }
            }
            Input::Audio(samples) => self.on_audio(&samples),
            Input::Transcribed { session, text } => {
                if self.current_session == Some(session) {
                    self.delivery.push_chunk(&text, &mut self.platform, now);
                    self.chunk_returned(now);
                }
            }
            Input::TranscribeFailed { session } => {
                if self.current_session == Some(session) {
                    self.platform.notify(Notice::ChunkFailed);
                    self.chunk_returned(now);
                }
            }
            Input::Tick => self.on_tick(now),
        }
        self.refresh_view(now);
    }

    fn is_recording(&self) -> bool {
        matches!(self.state, State::RecHold | State::RecToggle)
    }

    fn apply(&mut self, event: Event, now: Instant) {
        let (state, actions) = machine::step(self.state, event, now);
        self.state = state;
        for action in actions {
            match action {
                Action::StartRec => self.start_recording(now),
                Action::StopRec => self.stop_recording(),
                Action::Discard => self.discard(now),
                Action::Cancel => self.cancel(),
                Action::Finish => self.finish(now),
                Action::Show(message) => self.show_message(message),
            }
        }
        self.finish_if_drained(now);
    }

    fn start_recording(&mut self, now: Instant) {
        if !self.platform.stt_prepare() {
            self.state = State::Idle;
            self.platform.need_setup();
            return;
        }
        self.last_session += 1;
        self.current_session = Some(self.last_session);
        self.in_flight = 0;
        self.announced = false;
        self.mic_ready = false;
        self.started_at = Some(now);
        self.delivery = Delivery::new();
        self.chunker = Some(Chunker::new((self.make_vad)(), Config::default()));
        self.platform.capture_target();
        self.platform.capture_esc(true);
        self.platform.mic_start();
    }

    fn stop_recording(&mut self) {
        self.platform.mic_stop();
        self.platform.cue(Cue::Stop);
        if let Some(chunk) = self.chunker.as_mut().and_then(Chunker::flush) {
            self.queue(chunk);
        }
        self.chunker = None;
    }

    fn discard(&mut self, now: Instant) {
        self.platform.mic_stop();
        self.platform.stt_clear();
        let finished = self.delivery.finish(&mut self.platform, now);
        if !finished.text.is_empty() {
            self.platform.set_last_text(&finished.text);
        }
        self.end_session();
    }

    fn cancel(&mut self) {
        self.platform.mic_stop();
        self.platform.cue(Cue::Stop);
        self.platform.stt_clear();
        if self.delivery.cancel(&mut self.platform) == CancelOutcome::Refused {
            self.platform.notify(Notice::RemovalRefused);
        }
        self.end_session();
    }

    fn finish(&mut self, now: Instant) {
        let finished = self.delivery.finish(&mut self.platform, now);
        if !finished.text.is_empty() {
            self.platform.set_last_text(&finished.text);
        }
        if finished.undelivered {
            self.platform.notify(Notice::TextInMenu);
        } else if !finished.text.is_empty() {
            self.platform.submit();
        } else if !self.announced {
            self.platform.notify(Notice::NoSpeech);
        }
        self.end_session();
    }

    fn end_session(&mut self) {
        self.platform.capture_esc(false);
        self.current_session = None;
        self.chunker = None;
        self.in_flight = 0;
        self.started_at = None;
        self.mic_ready = false;
    }

    fn show_message(&mut self, message: Message) {
        let notice = match message {
            Message::StillTranscribing => Notice::StillTranscribing,
            Message::SilenceStopped => Notice::SilenceStopped,
            Message::LimitReached => Notice::LimitReached,
            Message::MicLost => Notice::MicLost,
        };
        self.announced |= notice != Notice::StillTranscribing;
        self.platform.notify(notice);
    }

    fn queue(&mut self, chunk: Vec<f32>) {
        if let Some(session) = self.current_session {
            self.in_flight += 1;
            self.platform.stt_transcribe(session, chunk);
        }
    }

    fn on_audio(&mut self, samples: &[f32]) {
        if !self.is_recording() {
            return;
        }
        let chunks = match self.chunker.as_mut() {
            Some(chunker) => chunker.push(samples),
            None => return,
        };
        for chunk in chunks {
            self.queue(chunk);
        }
    }

    fn chunk_returned(&mut self, now: Instant) {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.finish_if_drained(now);
    }

    fn finish_if_drained(&mut self, now: Instant) {
        if self.state == State::Draining && self.in_flight == 0 {
            self.apply(Event::DrainDone, now);
        }
    }

    fn on_tick(&mut self, now: Instant) {
        if let State::Armed { since } = self.state {
            if now.duration_since(since) >= FN_HOLD_DELAY {
                self.apply(Event::HoldElapsed, now);
            }
            return;
        }
        self.delivery.poll(&mut self.platform, now);
        let silence_ms = self.chunker.as_ref().map_or(0, Chunker::silence_ms);
        if self.state == State::RecToggle && silence_ms >= SILENCE_LIMIT_MS {
            self.apply(Event::SilenceTimeout, now);
        }
        let over_limit = self
            .started_at
            .is_some_and(|start| now.duration_since(start) >= MAX_RECORDING);
        if over_limit {
            self.apply(Event::MaxDuration, now);
        }
    }

    fn refresh_view(&mut self, now: Instant) {
        let waiting = self.delivery.has_pending();
        let view = match self.state {
            State::Idle | State::Armed { .. } => View::Hidden,
            State::RecHold | State::RecToggle if !self.mic_ready => View::Starting,
            State::RecHold | State::RecToggle => View::Recording {
                elapsed_s: self
                    .started_at
                    .map_or(0, |start| now.duration_since(start).as_secs()),
                waiting,
            },
            State::Draining => View::Transcribing { waiting },
        };
        if view != self.view {
            self.view = view;
            self.platform.show(view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunker::SAMPLE_RATE;
    use Input::*;

    #[derive(Default)]
    struct World {
        model_missing: bool,
        away: bool,
        interacted: bool,
        typed: String,
        backspaces: usize,
        targets: usize,
        mic_on: bool,
        mic_starts: usize,
        clears: usize,
        prepared: usize,
        queued: Vec<(u64, usize)>,
        views: Vec<View>,
        notices: Vec<Notice>,
        last_text: Option<String>,
        esc: bool,
        cues: Vec<Cue>,
        setups: usize,
        submits: usize,
    }

    impl Output for World {
        fn capture_target(&mut self) {
            self.targets += 1;
        }
        fn is_target_focused(&self) -> bool {
            !self.away
        }
        fn secure_input(&self) -> bool {
            false
        }
        fn type_text(&mut self, text: &str) {
            self.typed.push_str(text);
        }
        fn backspace(&mut self, count: usize) {
            self.backspaces += count;
        }
        fn user_interacted_since(&self, _since: Instant) -> bool {
            self.interacted
        }
        fn submit(&mut self) {
            self.submits += 1;
        }
    }

    impl Platform for World {
        fn mic_start(&mut self) {
            self.mic_on = true;
            self.mic_starts += 1;
        }
        fn mic_stop(&mut self) {
            self.mic_on = false;
        }
        fn stt_prepare(&mut self) -> bool {
            self.prepared += 1;
            !self.model_missing
        }
        fn stt_transcribe(&mut self, session: u64, audio: Vec<f32>) {
            self.queued.push((session, audio.len()));
        }
        fn stt_clear(&mut self) {
            self.clears += 1;
        }
        fn show(&mut self, view: View) {
            self.views.push(view);
        }
        fn notify(&mut self, notice: Notice) {
            self.notices.push(notice);
        }
        fn set_last_text(&mut self, text: &str) {
            self.last_text = Some(text.to_string());
        }
        fn capture_esc(&mut self, on: bool) {
            self.esc = on;
        }
        fn cue(&mut self, cue: Cue) {
            self.cues.push(cue);
        }
        fn need_setup(&mut self) {
            self.setups += 1;
        }
    }

    struct LoudVad;
    impl Vad for LoudVad {
        fn is_speech(&mut self, frame: &[f32]) -> bool {
            frame.iter().any(|s| s.abs() > 0.5)
        }
    }

    struct Rig {
        engine: Engine<World, LoudVad>,
        now: Instant,
    }

    impl Rig {
        fn new() -> Self {
            Self::with(World::default())
        }

        fn with(world: World) -> Self {
            Self {
                engine: Engine::new(world, || LoudVad),
                now: Instant::now(),
            }
        }

        fn send(&mut self, input: Input) {
            self.engine.handle(input, self.now);
        }

        fn wait(&mut self, ms: u64) {
            self.now += Duration::from_millis(ms);
        }

        fn world(&self) -> &World {
            self.engine.platform()
        }

        fn speech(&mut self, ms: usize) {
            self.send(Audio(vec![0.9; ms * SAMPLE_RATE / 1000]));
            self.wait(ms as u64);
        }

        fn silence(&mut self, ms: usize) {
            self.send(Audio(vec![0.0; ms * SAMPLE_RATE / 1000]));
            self.wait(ms as u64);
        }

        fn view(&self) -> View {
            *self.world().views.last().expect("no view shown")
        }

        /// Presses `fn` and keeps it down until the delay ends.
        fn hold_fn(&mut self) {
            self.send(Key(Event::FnDown));
            self.wait(FN_HOLD_DELAY.as_millis() as u64);
            self.send(Tick);
        }

        fn start_hold(&mut self) {
            self.hold_fn();
            self.send(MicReady);
        }

        fn start_toggle(&mut self) {
            self.send(Key(Event::ToggleHotkey));
            self.send(MicReady);
        }

        /// Speech followed by a pause long enough to complete a chunk.
        fn spoken_chunk(&mut self) {
            self.speech(1_000);
            self.silence(800);
        }
    }

    #[test]
    fn holding_fn_past_the_delay_starts_a_session() {
        let mut r = Rig::new();
        r.hold_fn();
        let w = r.world();
        assert!(w.mic_on);
        assert_eq!(w.targets, 1);
        assert_eq!(w.prepared, 1);
        assert!(w.esc);
        assert_eq!(r.view(), View::Starting);
    }

    #[test]
    fn nothing_happens_during_the_delay() {
        let mut r = Rig::new();
        r.send(Key(Event::FnDown));
        r.wait(999);
        r.send(Tick);
        let w = r.world();
        assert!(!w.mic_on);
        assert_eq!(w.targets, 0);
        assert_eq!(w.prepared, 0);
        assert!(!w.esc);
        assert!(w.views.is_empty());
    }

    #[test]
    fn recording_starts_exactly_when_the_second_second_begins() {
        let mut r = Rig::new();
        r.send(Key(Event::FnDown));
        r.wait(999);
        r.send(Tick);
        assert!(!r.world().mic_on);
        r.wait(1);
        r.send(Tick);
        assert!(r.world().mic_on);
    }

    #[test]
    fn the_engine_asks_to_be_woken_when_the_delay_ends() {
        let mut r = Rig::new();
        assert_eq!(r.engine.hold_deadline(), None);
        r.send(Key(Event::FnDown));
        assert_eq!(r.engine.hold_deadline(), Some(r.now + FN_HOLD_DELAY));
        r.send(Key(Event::FnUp));
        assert_eq!(r.engine.hold_deadline(), None);
    }

    #[test]
    fn a_short_fn_press_leaves_no_trace() {
        let mut r = Rig::new();
        r.send(Key(Event::FnDown));
        r.wait(300);
        r.send(Tick);
        r.send(Key(Event::FnUp));
        r.wait(3_000);
        r.send(Tick);
        let w = r.world();
        assert_eq!(w.mic_starts, 0);
        assert_eq!(w.prepared, 0);
        assert!(w.views.is_empty());
        assert!(w.notices.is_empty());
    }

    #[test]
    fn a_fn_chord_cancels_the_delay() {
        let mut r = Rig::new();
        r.send(Key(Event::FnDown));
        r.send(Key(Event::FnChord));
        r.wait(3_000);
        r.send(Tick);
        assert_eq!(r.world().mic_starts, 0);
    }

    #[test]
    fn releasing_fn_without_speaking_says_so() {
        let mut r = Rig::new();
        r.start_hold();
        r.silence(500);
        r.send(Key(Event::FnUp));
        assert_eq!(r.world().notices, vec![Notice::NoSpeech]);
        assert_eq!(r.view(), View::Hidden);
    }

    #[test]
    fn missing_model_opens_setup_instead_of_recording() {
        let mut r = Rig::with(World {
            model_missing: true,
            ..World::default()
        });
        r.hold_fn();
        assert!(!r.world().mic_on);
        assert!(!r.world().esc);
        assert_eq!(r.world().setups, 1);
        r.send(Key(Event::FnUp));
        r.send(Key(Event::ToggleHotkey));
        assert_eq!(r.world().setups, 2);
        assert_eq!(r.world().mic_starts, 0);
    }

    #[test]
    fn mic_ready_switches_to_recording_view_and_plays_the_cue() {
        let mut r = Rig::new();
        r.start_hold();
        assert_eq!(
            r.view(),
            View::Recording {
                elapsed_s: 0,
                waiting: false
            }
        );
        assert_eq!(r.world().cues, vec![Cue::Start]);
    }

    #[test]
    fn a_finished_chunk_goes_to_transcription_while_recording_continues() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        assert_eq!(r.world().queued.len(), 1);
        assert_eq!(r.world().queued[0].0, 1);
        assert!(r.world().mic_on);
    }

    #[test]
    fn transcribed_text_is_typed_during_recording() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.send(Transcribed {
            session: 1,
            text: "salom dunyo".into(),
        });
        assert_eq!(r.world().typed, "Salom dunyo");
        assert!(r.world().mic_on);
    }

    #[test]
    fn stopping_flushes_audio_and_waits_for_the_last_result() {
        let mut r = Rig::new();
        r.start_hold();
        r.speech(1_000);
        r.send(Key(Event::FnUp));
        assert!(!r.world().mic_on);
        assert_eq!(r.world().queued.len(), 1);
        assert_eq!(r.view(), View::Transcribing { waiting: false });
        assert!(r.world().esc, "Esc still cancels while draining");
        assert_eq!(r.world().last_text, None);

        r.send(Transcribed {
            session: 1,
            text: "salom".into(),
        });
        let w = r.world();
        assert_eq!(w.typed, "Salom ");
        assert_eq!(w.last_text.as_deref(), Some("Salom"));
        assert!(!w.esc);
        assert_eq!(r.view(), View::Hidden);
        assert_eq!(w.cues, vec![Cue::Start, Cue::Stop]);
    }

    #[test]
    fn session_ends_only_after_every_queued_chunk_returned() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.spoken_chunk();
        r.send(Key(Event::ToggleHotkey));
        assert_eq!(r.world().queued.len(), 2);
        r.send(Transcribed {
            session: 1,
            text: "bir".into(),
        });
        assert_eq!(r.world().last_text, None);
        r.send(Transcribed {
            session: 1,
            text: "ikki".into(),
        });
        assert_eq!(r.world().last_text.as_deref(), Some("Bir ikki"));
    }

    #[test]
    fn stopping_without_speech_says_so() {
        let mut r = Rig::new();
        r.start_toggle();
        r.silence(1_000);
        r.send(Key(Event::ToggleHotkey));
        assert_eq!(r.world().notices, vec![Notice::NoSpeech]);
        assert_eq!(r.world().queued.len(), 0);
        assert_eq!(r.view(), View::Hidden);
    }

    #[test]
    fn esc_removes_typed_text_and_drops_the_queue() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.send(Transcribed {
            session: 1,
            text: "salom".into(),
        });
        r.spoken_chunk();
        r.send(Key(Event::Esc));
        let w = r.world();
        assert_eq!(w.backspaces, "Salom".len());
        assert_eq!(w.clears, 1);
        assert!(!w.mic_on);
        assert!(!w.esc);
        assert_eq!(w.last_text, None);
        assert_eq!(r.view(), View::Hidden);
    }

    #[test]
    fn esc_reports_when_removal_was_refused() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.send(Transcribed {
            session: 1,
            text: "salom".into(),
        });
        r.engine.platform_mut().interacted = true;
        r.send(Key(Event::Esc));
        assert_eq!(r.world().backspaces, 0);
        assert_eq!(r.world().notices, vec![Notice::RemovalRefused]);
    }

    #[test]
    fn results_from_a_cancelled_session_are_ignored() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.send(Key(Event::Esc));
        r.start_toggle();
        r.spoken_chunk();
        assert_eq!(r.world().queued.last().unwrap().0, 2);
        r.send(Transcribed {
            session: 1,
            text: "eski".into(),
        });
        assert_eq!(r.world().typed, "");
        r.send(Transcribed {
            session: 2,
            text: "yangi".into(),
        });
        assert_eq!(r.world().typed, "Yangi");
    }

    #[test]
    fn audio_arriving_after_stop_is_ignored() {
        let mut r = Rig::new();
        r.start_toggle();
        r.send(Key(Event::ToggleHotkey));
        r.speech(1_000);
        r.silence(800);
        assert!(r.world().queued.is_empty());
    }

    #[test]
    fn toggle_recording_stops_after_thirty_seconds_of_silence() {
        let mut r = Rig::new();
        r.start_toggle();
        r.silence(29_000);
        r.send(Tick);
        assert!(r.world().mic_on);
        r.silence(1_100);
        r.send(Tick);
        assert!(!r.world().mic_on);
        assert_eq!(r.world().notices, vec![Notice::SilenceStopped]);
    }

    #[test]
    fn speech_restarts_the_silence_timer() {
        let mut r = Rig::new();
        r.start_toggle();
        r.silence(25_000);
        r.speech(500);
        r.silence(25_000);
        r.send(Tick);
        assert!(r.world().mic_on);
    }

    #[test]
    fn holding_fn_never_stops_on_silence() {
        let mut r = Rig::new();
        r.start_hold();
        r.silence(40_000);
        r.send(Tick);
        assert!(r.world().mic_on);
    }

    #[test]
    fn recording_stops_at_the_time_limit() {
        let mut r = Rig::new();
        r.start_hold();
        r.wait(MAX_RECORDING.as_millis() as u64 + 1);
        r.send(Tick);
        assert!(!r.world().mic_on);
        assert_eq!(r.world().notices, vec![Notice::LimitReached]);
    }

    #[test]
    fn losing_the_microphone_keeps_what_was_said() {
        let mut r = Rig::new();
        r.start_toggle();
        r.speech(1_000);
        r.send(Key(Event::DeviceLost));
        assert_eq!(r.world().notices, vec![Notice::MicLost]);
        assert_eq!(r.world().queued.len(), 1);
    }

    #[test]
    fn failed_chunk_is_reported_and_the_session_continues() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.spoken_chunk();
        r.send(Key(Event::ToggleHotkey));
        r.send(TranscribeFailed { session: 1 });
        assert_eq!(r.world().notices, vec![Notice::ChunkFailed]);
        r.send(Transcribed {
            session: 1,
            text: "ikki".into(),
        });
        assert_eq!(r.world().typed, "Ikki ");
        assert_eq!(r.view(), View::Hidden);
    }

    #[test]
    fn waiting_text_is_shown_and_typed_when_focus_returns() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.engine.platform_mut().away = true;
        r.send(Transcribed {
            session: 1,
            text: "salom".into(),
        });
        r.send(Tick);
        assert_eq!(
            r.view(),
            View::Recording {
                elapsed_s: 1,
                waiting: true
            }
        );
        r.engine.platform_mut().away = false;
        r.send(Tick);
        assert_eq!(r.world().typed, "Salom");
        assert_eq!(
            r.view(),
            View::Recording {
                elapsed_s: 1,
                waiting: false
            }
        );
    }

    #[test]
    fn undelivered_text_stays_in_the_menu() {
        let mut r = Rig::new();
        r.start_toggle();
        r.spoken_chunk();
        r.engine.platform_mut().away = true;
        r.send(Key(Event::ToggleHotkey));
        r.send(Transcribed {
            session: 1,
            text: "salom".into(),
        });
        let w = r.world();
        assert_eq!(w.typed, "");
        assert_eq!(w.last_text.as_deref(), Some("Salom"));
        assert_eq!(w.notices, vec![Notice::TextInMenu]);
    }

    #[test]
    fn only_a_fully_delivered_session_is_submitted() {
        let mut r = Rig::new();
        r.start_hold();
        r.speech(1_000);
        r.send(Key(Event::FnUp));
        r.send(Transcribed {
            session: 1,
            text: "salom".into(),
        });
        assert_eq!(r.world().submits, 1);

        r.start_toggle();
        r.spoken_chunk();
        r.send(Key(Event::Esc));
        assert_eq!(r.world().submits, 1, "cancelled");

        r.start_toggle();
        r.spoken_chunk();
        r.engine.platform_mut().away = true;
        r.send(Key(Event::ToggleHotkey));
        r.send(Transcribed {
            session: 3,
            text: "salom".into(),
        });
        assert_eq!(r.world().submits, 1, "undelivered");

        r.start_toggle();
        r.send(Key(Event::ToggleHotkey));
        assert_eq!(r.world().submits, 1, "no speech");
    }

    #[test]
    fn a_new_recording_is_refused_until_transcription_ends() {
        let mut r = Rig::new();
        r.start_hold();
        r.speech(1_000);
        r.send(Key(Event::FnUp));
        r.send(Key(Event::FnDown));
        assert_eq!(r.world().notices, vec![Notice::StillTranscribing]);
        assert_eq!(r.world().mic_starts, 1);
    }

    #[test]
    fn elapsed_time_is_published_once_per_second() {
        let mut r = Rig::new();
        r.start_toggle();
        let before = r.world().views.len();
        r.wait(300);
        r.send(Tick);
        r.wait(300);
        r.send(Tick);
        assert_eq!(r.world().views.len(), before);
        r.wait(500);
        r.send(Tick);
        assert_eq!(r.world().views.len(), before + 1);
        assert_eq!(
            r.view(),
            View::Recording {
                elapsed_s: 1,
                waiting: false
            }
        );
    }

    #[test]
    fn a_second_session_starts_from_a_clean_state() {
        let mut r = Rig::new();
        r.start_hold();
        r.speech(1_000);
        r.send(Key(Event::FnUp));
        r.send(Transcribed {
            session: 1,
            text: "bir".into(),
        });
        r.start_hold();
        r.speech(1_000);
        r.send(Key(Event::FnUp));
        assert_eq!(r.world().queued.last().unwrap().0, 2);
        r.send(Transcribed {
            session: 2,
            text: "ikki".into(),
        });
        assert_eq!(r.world().typed, "Bir Ikki ");
        assert_eq!(r.world().last_text.as_deref(), Some("Ikki"));
    }
}
