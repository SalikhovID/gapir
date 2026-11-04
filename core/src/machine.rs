//! Recording state machine. Pure: time comes in as a parameter and effects go out as actions.

use std::time::{Duration, Instant};

/// A shorter `fn` press is treated as an accidental tap, not as dictation.
pub const MIN_HOLD: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Recording while `fn` is held down.
    RecHold {
        since: Instant,
    },
    /// Recording until the toggle hotkey is pressed again.
    RecToggle,
    /// Microphone closed; the queued audio is still being transcribed.
    Draining,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    FnDown,
    FnUp,
    /// `fn` was used as a modifier for another key, so this is not dictation.
    FnChord,
    ToggleHotkey,
    Esc,
    SilenceTimeout,
    MaxDuration,
    DeviceLost,
    DrainDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    StillTranscribing,
    SilenceStopped,
    LimitReached,
    MicLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the microphone, remember the target window, capture Esc.
    StartRec,
    /// Close the microphone and queue the remaining audio.
    StopRec,
    /// Close the microphone and drop queued audio; typed text stays.
    Discard,
    /// `Discard`, and remove the typed text if that is still safe.
    Cancel,
    /// Finish the session: trailing space, hide the overlay, release Esc.
    Finish,
    Show(Message),
}

pub fn step(state: State, event: Event, now: Instant) -> (State, Vec<Action>) {
    use Action::*;
    use Event::*;
    use State::*;

    match (state, event) {
        (Idle, FnDown) => (RecHold { since: now }, vec![StartRec]),
        (Idle, ToggleHotkey) => (RecToggle, vec![StartRec]),

        (RecHold { since }, FnUp) if now.duration_since(since) < MIN_HOLD => (Idle, vec![Discard]),
        (RecHold { .. }, FnUp) => (Draining, vec![StopRec]),
        (RecHold { .. }, FnChord) => (Idle, vec![Discard]),

        (RecToggle, ToggleHotkey) => (Draining, vec![StopRec]),
        (RecToggle, SilenceTimeout) => (Draining, vec![StopRec, Show(Message::SilenceStopped)]),

        (RecHold { .. } | RecToggle, Esc) | (Draining, Esc) => (Idle, vec![Cancel]),
        (RecHold { .. } | RecToggle, MaxDuration) => {
            (Draining, vec![StopRec, Show(Message::LimitReached)])
        }
        (RecHold { .. } | RecToggle, DeviceLost) => {
            (Draining, vec![StopRec, Show(Message::MicLost)])
        }

        (Draining, DrainDone) => (Idle, vec![Finish]),
        (Draining, FnDown | ToggleHotkey) => (Draining, vec![Show(Message::StillTranscribing)]),

        (state, _) => (state, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;
    use Event::*;
    use State::*;

    fn at(ms: u64) -> (Instant, Instant) {
        let start = Instant::now();
        (start, start + Duration::from_millis(ms))
    }

    #[test]
    fn idle_fn_down_starts_hold_recording() {
        let (now, _) = at(0);
        assert_eq!(
            step(Idle, FnDown, now),
            (RecHold { since: now }, vec![StartRec])
        );
    }

    #[test]
    fn idle_toggle_starts_toggle_recording() {
        let (now, _) = at(0);
        assert_eq!(step(Idle, ToggleHotkey, now), (RecToggle, vec![StartRec]));
    }

    #[test]
    fn idle_fn_up_is_ignored() {
        let (now, _) = at(0);
        assert_eq!(step(Idle, FnUp, now), (Idle, vec![]));
    }

    #[test]
    fn idle_ignores_every_other_event() {
        let (now, _) = at(0);
        for e in [
            FnChord,
            Esc,
            SilenceTimeout,
            MaxDuration,
            DeviceLost,
            DrainDone,
        ] {
            assert_eq!(step(Idle, e, now), (Idle, vec![]), "{e:?}");
        }
    }

    #[test]
    fn quick_fn_tap_discards() {
        let (start, later) = at(199);
        assert_eq!(
            step(RecHold { since: start }, FnUp, later),
            (Idle, vec![Discard])
        );
    }

    #[test]
    fn fn_release_after_threshold_stops_and_drains() {
        let (start, later) = at(200);
        assert_eq!(
            step(RecHold { since: start }, FnUp, later),
            (Draining, vec![StopRec])
        );
    }

    #[test]
    fn fn_chord_discards() {
        let (start, later) = at(800);
        assert_eq!(
            step(RecHold { since: start }, FnChord, later),
            (Idle, vec![Discard])
        );
    }

    #[test]
    fn esc_during_hold_cancels() {
        let (start, later) = at(800);
        assert_eq!(
            step(RecHold { since: start }, Esc, later),
            (Idle, vec![Cancel])
        );
    }

    #[test]
    fn toggle_during_hold_is_ignored() {
        let (start, later) = at(800);
        let state = RecHold { since: start };
        assert_eq!(step(state, ToggleHotkey, later), (state, vec![]));
    }

    #[test]
    fn hold_ignores_silence_timeout() {
        let (start, later) = at(31_000);
        let state = RecHold { since: start };
        assert_eq!(step(state, SilenceTimeout, later), (state, vec![]));
    }

    #[test]
    fn second_toggle_stops_and_drains() {
        let (now, _) = at(0);
        assert_eq!(
            step(RecToggle, ToggleHotkey, now),
            (Draining, vec![StopRec])
        );
    }

    #[test]
    fn esc_during_toggle_cancels() {
        let (now, _) = at(0);
        assert_eq!(step(RecToggle, Esc, now), (Idle, vec![Cancel]));
    }

    #[test]
    fn silence_timeout_stops_toggle_recording() {
        let (now, _) = at(0);
        assert_eq!(
            step(RecToggle, SilenceTimeout, now),
            (Draining, vec![StopRec, Show(Message::SilenceStopped)])
        );
    }

    #[test]
    fn fn_events_do_not_affect_toggle_recording() {
        let (now, _) = at(0);
        for e in [FnDown, FnUp, FnChord] {
            assert_eq!(step(RecToggle, e, now), (RecToggle, vec![]), "{e:?}");
        }
    }

    #[test]
    fn limit_and_device_loss_stop_both_recording_modes() {
        let (start, later) = at(5_000);
        for state in [RecHold { since: start }, RecToggle] {
            assert_eq!(
                step(state, MaxDuration, later),
                (Draining, vec![StopRec, Show(Message::LimitReached)])
            );
            assert_eq!(
                step(state, DeviceLost, later),
                (Draining, vec![StopRec, Show(Message::MicLost)])
            );
        }
    }

    #[test]
    fn drain_done_finishes() {
        let (now, _) = at(0);
        assert_eq!(step(Draining, DrainDone, now), (Idle, vec![Finish]));
    }

    #[test]
    fn esc_during_drain_cancels() {
        let (now, _) = at(0);
        assert_eq!(step(Draining, Esc, now), (Idle, vec![Cancel]));
    }

    #[test]
    fn new_recording_is_refused_while_draining() {
        let (now, _) = at(0);
        for e in [FnDown, ToggleHotkey] {
            assert_eq!(
                step(Draining, e, now),
                (Draining, vec![Show(Message::StillTranscribing)]),
                "{e:?}"
            );
        }
    }

    #[test]
    fn draining_ignores_late_recording_events() {
        let (now, _) = at(0);
        for e in [FnUp, FnChord, SilenceTimeout, MaxDuration, DeviceLost] {
            assert_eq!(step(Draining, e, now), (Draining, vec![]), "{e:?}");
        }
    }
}
