//! Decides when transcribed text may enter the target window, and whether it may be removed
//! again on cancel. The OS side lives behind [`Output`].

use crate::textfmt;
use std::time::Instant;
use unicode_segmentation::UnicodeSegmentation;

pub trait Output {
    /// The window that was focused when recording started still has focus.
    fn is_target_focused(&self) -> bool;
    /// macOS secure input is on (password field), so typed text would be swallowed.
    fn secure_input(&self) -> bool;
    fn type_text(&mut self, text: &str);
    fn backspace(&mut self, count: usize);
    /// The user pressed a key or the mouse after `since`, so the cursor may have moved.
    fn user_interacted_since(&self, since: Instant) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelOutcome {
    /// Nothing had been typed yet.
    NothingToRemove,
    Removed,
    /// Removal was refused because the cursor may not be where we left it.
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// Whole session text, for "copy last text" in the menu.
    pub text: String,
    /// Part of the text never reached the target window.
    pub undelivered: bool,
}

/// One dictation session. Text is appended here and leaves through [`Output`] in order.
#[derive(Default)]
pub struct Delivery {
    /// Whole session text.
    full: String,
    /// Tail of `full` that has not reached the target window yet.
    pending: String,
    typed_graphemes: usize,
    first_typed_at: Option<Instant>,
}

impl Delivery {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a transcribed chunk and types it if the target window is ready.
    pub fn push_chunk(&mut self, raw: &str, out: &mut impl Output, now: Instant) {
        if textfmt::is_hallucination(raw) {
            return;
        }
        let is_first = self.full.is_empty();
        let text = textfmt::format_chunk(raw, is_first || textfmt::ends_sentence(&self.full));
        if text.is_empty() {
            return;
        }
        if !is_first {
            self.full.push(' ');
            self.pending.push(' ');
        }
        self.full.push_str(&text);
        self.pending.push_str(&text);
        self.poll(out, now);
    }

    /// Called periodically: types waiting text once the target window is back.
    pub fn poll(&mut self, out: &mut impl Output, now: Instant) {
        if self.pending.is_empty() || !out.is_target_focused() || out.secure_input() {
            return;
        }
        let text = std::mem::take(&mut self.pending);
        out.type_text(&text);
        self.typed_graphemes += text.graphemes(true).count();
        self.first_typed_at.get_or_insert(now);
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Ends the session. A trailing space lets the next dictation continue the text.
    pub fn finish(&mut self, out: &mut impl Output, now: Instant) -> Finished {
        self.poll(out, now);
        let undelivered = self.has_pending();
        if !undelivered && !self.full.is_empty() {
            out.type_text(" ");
        }
        let text = std::mem::take(&mut self.full);
        *self = Self::default();
        Finished { text, undelivered }
    }

    /// Ends the session and removes the typed text, unless the cursor may have moved:
    /// blind backspaces there would delete the user's own text.
    pub fn cancel(&mut self, out: &mut impl Output) -> CancelOutcome {
        let outcome = match self.first_typed_at {
            None => CancelOutcome::NothingToRemove,
            Some(since)
                if !out.is_target_focused()
                    || out.secure_input()
                    || out.user_interacted_since(since) =>
            {
                CancelOutcome::Refused
            }
            Some(_) => {
                out.backspace(self.typed_graphemes);
                CancelOutcome::Removed
            }
        };
        *self = Self::default();
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        away: bool,
        secure: bool,
        interacted: bool,
        typed: String,
        backspaces: usize,
    }

    impl Output for Fake {
        fn is_target_focused(&self) -> bool {
            !self.away
        }
        fn secure_input(&self) -> bool {
            self.secure
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
    }

    fn setup() -> (Delivery, Fake, Instant) {
        (Delivery::new(), Fake::default(), Instant::now())
    }

    #[test]
    fn focused_chunk_is_typed_immediately_with_a_capital() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom dunyo", &mut out, now);
        assert_eq!(out.typed, "Salom dunyo");
        assert!(!d.has_pending());
    }

    #[test]
    fn chunks_are_joined_by_one_space_without_a_new_capital() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom", &mut out, now);
        d.push_chunk("dunyo", &mut out, now);
        assert_eq!(out.typed, "Salom dunyo");
    }

    #[test]
    fn chunk_after_a_finished_sentence_starts_with_a_capital() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom.", &mut out, now);
        d.push_chunk("dunyo", &mut out, now);
        assert_eq!(out.typed, "Salom. Dunyo");
    }

    #[test]
    fn empty_and_hallucinated_chunks_are_skipped() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("   ", &mut out, now);
        d.push_chunk("musiqa", &mut out, now);
        d.push_chunk("salom", &mut out, now);
        assert_eq!(out.typed, "Salom");
    }

    #[test]
    fn text_waits_while_focus_is_elsewhere() {
        let (mut d, mut out, now) = setup();
        out.away = true;
        d.push_chunk("salom", &mut out, now);
        assert_eq!(out.typed, "");
        assert!(d.has_pending());
    }

    #[test]
    fn waiting_text_is_typed_in_order_when_focus_returns() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("bir", &mut out, now);
        out.away = true;
        d.push_chunk("ikki", &mut out, now);
        d.push_chunk("uch", &mut out, now);
        d.poll(&mut out, now);
        assert_eq!(out.typed, "Bir");
        out.away = false;
        d.poll(&mut out, now);
        assert_eq!(out.typed, "Bir ikki uch");
        assert!(!d.has_pending());
    }

    #[test]
    fn a_new_chunk_never_overtakes_waiting_text() {
        let (mut d, mut out, now) = setup();
        out.away = true;
        d.push_chunk("bir", &mut out, now);
        out.away = false;
        d.push_chunk("ikki", &mut out, now);
        assert_eq!(out.typed, "Bir ikki");
    }

    #[test]
    fn secure_input_holds_text_back() {
        let (mut d, mut out, now) = setup();
        out.secure = true;
        d.push_chunk("parol", &mut out, now);
        assert_eq!(out.typed, "");
        assert!(d.has_pending());
    }

    #[test]
    fn finish_adds_a_trailing_space_for_the_next_dictation() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom", &mut out, now);
        let done = d.finish(&mut out, now);
        assert_eq!(out.typed, "Salom ");
        assert_eq!(
            done,
            Finished {
                text: "Salom".into(),
                undelivered: false
            }
        );
    }

    #[test]
    fn finish_without_text_types_nothing() {
        let (mut d, mut out, now) = setup();
        let done = d.finish(&mut out, now);
        assert_eq!(out.typed, "");
        assert_eq!(
            done,
            Finished {
                text: String::new(),
                undelivered: false
            }
        );
    }

    #[test]
    fn finish_delivers_waiting_text_if_focus_is_back() {
        let (mut d, mut out, now) = setup();
        out.away = true;
        d.push_chunk("salom", &mut out, now);
        out.away = false;
        let done = d.finish(&mut out, now);
        assert_eq!(out.typed, "Salom ");
        assert!(!done.undelivered);
    }

    #[test]
    fn undelivered_text_is_reported_with_the_whole_session() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("bir", &mut out, now);
        out.away = true;
        d.push_chunk("ikki", &mut out, now);
        let done = d.finish(&mut out, now);
        assert_eq!(out.typed, "Bir");
        assert_eq!(
            done,
            Finished {
                text: "Bir ikki".into(),
                undelivered: true
            }
        );
    }

    #[test]
    fn cancel_removes_exactly_what_was_typed() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom", &mut out, now);
        d.push_chunk("dunyo", &mut out, now);
        assert_eq!(d.cancel(&mut out), CancelOutcome::Removed);
        assert_eq!(out.backspaces, "Salom dunyo".len());
    }

    #[test]
    fn cancel_counts_graphemes_not_bytes() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("şa👍🏽", &mut out, now);
        assert_eq!(d.cancel(&mut out), CancelOutcome::Removed);
        assert_eq!(out.backspaces, 3);
    }

    #[test]
    fn cancel_ignores_text_that_never_got_typed() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom", &mut out, now);
        out.away = true;
        d.push_chunk("dunyo", &mut out, now);
        out.away = false;
        assert_eq!(d.cancel(&mut out), CancelOutcome::Removed);
        assert_eq!(out.backspaces, "Salom".len());
        assert_eq!(out.typed, "Salom");
    }

    #[test]
    fn cancel_with_nothing_typed_does_nothing() {
        let (mut d, mut out, now) = setup();
        out.away = true;
        d.push_chunk("salom", &mut out, now);
        assert_eq!(d.cancel(&mut out), CancelOutcome::NothingToRemove);
        assert_eq!(out.backspaces, 0);
    }

    #[test]
    fn cancel_refuses_when_focus_moved() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom", &mut out, now);
        out.away = true;
        assert_eq!(d.cancel(&mut out), CancelOutcome::Refused);
        assert_eq!(out.backspaces, 0);
    }

    #[test]
    fn cancel_refuses_when_the_user_touched_keyboard_or_mouse() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom", &mut out, now);
        out.interacted = true;
        assert_eq!(d.cancel(&mut out), CancelOutcome::Refused);
        assert_eq!(out.backspaces, 0);
    }

    #[test]
    fn cancel_refuses_in_a_password_field() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("salom", &mut out, now);
        out.secure = true;
        assert_eq!(d.cancel(&mut out), CancelOutcome::Refused);
        assert_eq!(out.backspaces, 0);
    }

    #[test]
    fn a_second_session_starts_clean() {
        let (mut d, mut out, now) = setup();
        d.push_chunk("bir", &mut out, now);
        d.finish(&mut out, now);
        d.push_chunk("ikki", &mut out, now);
        assert_eq!(out.typed, "Bir Ikki");
    }
}
