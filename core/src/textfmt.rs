//! Cleans raw model output before it is typed: one apostrophe, sentence capitals, single spaces.

const APOSTROPHES: [char; 6] = ['ʻ', 'ʼ', '’', '‘', '`', '´'];
const SENTENCE_END: [char; 3] = ['.', '!', '?'];

/// Words the model produces for silence or noise instead of real speech.
const HALLUCINATIONS: [&str; 1] = ["musiqa"];

/// Formats one transcribed chunk. `capitalize_first` is true at the start of a session and
/// when the previous chunk ended a sentence. No punctuation is invented at pauses.
pub fn format_chunk(raw: &str, capitalize_first: bool) -> String {
    let normalized: String = raw
        .chars()
        .map(|c| if APOSTROPHES.contains(&c) { '\'' } else { c })
        .collect();

    let mut out = String::with_capacity(normalized.len());
    let mut capitalize = capitalize_first;
    for (index, word) in normalized.split_whitespace().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        if capitalize {
            push_capitalized(&mut out, word);
        } else {
            out.push_str(word);
        }
        capitalize = ends_sentence(word);
    }
    out
}

fn push_capitalized(out: &mut String, word: &str) {
    let mut chars = word.chars();
    if let Some(first) = chars.next() {
        out.extend(first.to_uppercase());
        out.push_str(chars.as_str());
    }
}

pub fn ends_sentence(text: &str) -> bool {
    text.trim_end().ends_with(SENTENCE_END)
}

/// True when the whole chunk is a known silence artifact. Real sentences containing the word pass.
pub fn is_hallucination(raw: &str) -> bool {
    let word: String = raw
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let is_single_word = raw.split_whitespace().count() == 1;
    is_single_word && HALLUCINATIONS.contains(&word.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_apostrophe_variant_becomes_ascii() {
        assert_eq!(
            format_chunk("oʻzbek gʼoya ma’no ta‘lim to`g´ri", false),
            "o'zbek g'oya ma'no ta'lim to'g'ri"
        );
    }

    #[test]
    fn first_letter_is_capitalized_when_requested() {
        assert_eq!(format_chunk("salom dunyo", true), "Salom dunyo");
        assert_eq!(format_chunk("salom dunyo", false), "salom dunyo");
    }

    #[test]
    fn digraph_with_apostrophe_keeps_its_shape() {
        assert_eq!(format_chunk("oʻzbek tili", true), "O'zbek tili");
        assert_eq!(format_chunk("g'oya", true), "G'oya");
        assert_eq!(format_chunk("shahar", true), "Shahar");
    }

    #[test]
    fn letter_after_sentence_end_is_capitalized() {
        assert_eq!(format_chunk("salom. dunyo", false), "salom. Dunyo");
        assert_eq!(format_chunk("nima? ha! yo'q", true), "Nima? Ha! Yo'q");
    }

    #[test]
    fn punctuation_inside_a_token_does_not_capitalize() {
        assert_eq!(
            format_chunk("narxi 3.5 foiz bo'ldi", false),
            "narxi 3.5 foiz bo'ldi"
        );
        assert_eq!(
            format_chunk("sayt gapir.uz ochildi", false),
            "sayt gapir.uz ochildi"
        );
    }

    #[test]
    fn whitespace_is_collapsed_and_trimmed() {
        assert_eq!(format_chunk("  salom \n\t dunyo  ", false), "salom dunyo");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(format_chunk("   ", true), "");
    }

    #[test]
    fn no_period_is_added_at_a_pause() {
        assert_eq!(format_chunk("gapni tugatmadim", false), "gapni tugatmadim");
    }

    #[test]
    fn digits_at_start_are_left_alone() {
        assert_eq!(format_chunk("5 kishi keldi", true), "5 kishi keldi");
    }

    #[test]
    fn sentence_end_detection() {
        assert!(ends_sentence("salom."));
        assert!(ends_sentence("nima?"));
        assert!(ends_sentence("ha!"));
        assert!(!ends_sentence("salom"));
        assert!(!ends_sentence("salom,"));
        assert!(!ends_sentence(""));
    }

    #[test]
    fn known_silence_hallucination_is_detected() {
        assert!(is_hallucination("musiqa"));
        assert!(is_hallucination("  Musiqa. "));
        assert!(is_hallucination("[musiqa]"));
    }

    #[test]
    fn real_sentences_with_the_word_are_not_hallucinations() {
        assert!(!is_hallucination("musiqa eshityapman"));
        assert!(!is_hallucination("salom"));
        assert!(!is_hallucination(""));
    }
}
