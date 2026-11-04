//! Splits a live audio stream into chunks at speech pauses so text can be delivered while the
//! user keeps talking. Pure over a [`Vad`], so behaviour is testable with synthetic audio.

pub const SAMPLE_RATE: usize = 16_000;
/// Samples per voice activity frame (16 ms), the size the VAD expects.
pub const FRAME: usize = 256;

pub trait Vad {
    fn is_speech(&mut self, frame: &[f32]) -> bool;
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Silence after speech that ends a chunk.
    pub pause_ms: usize,
    /// Longest allowed chunk, kept below Whisper's 30 s window.
    pub max_chunk_ms: usize,
    /// Pause length preferred as a cut point when a chunk gets too long.
    pub soft_pause_ms: usize,
    /// Chunks with less speech than this are dropped: they only make the model hallucinate.
    pub min_speech_ms: usize,
    /// Audio kept on each side of the speech.
    pub padding_ms: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pause_ms: 700,
            max_chunk_ms: 25_000,
            soft_pause_ms: 150,
            min_speech_ms: 200,
            padding_ms: 200,
        }
    }
}

/// Voice activity detection backed by the `earshot` neural network.
pub struct EarshotVad {
    detector: Box<earshot::Detector>,
}

impl EarshotVad {
    const THRESHOLD: f32 = 0.5;

    pub fn new() -> Self {
        Self {
            detector: earshot::Detector::default_boxed(),
        }
    }
}

impl Default for EarshotVad {
    fn default() -> Self {
        Self::new()
    }
}

impl Vad for EarshotVad {
    fn is_speech(&mut self, frame: &[f32]) -> bool {
        self.detector.predict_f32(frame) >= Self::THRESHOLD
    }
}

const FRAME_MS: usize = FRAME * 1000 / SAMPLE_RATE;

fn frames_in(ms: usize) -> usize {
    ms.div_ceil(FRAME_MS)
}

pub struct Chunker<V: Vad> {
    vad: V,
    pause_frames: usize,
    max_frames: usize,
    soft_pause_frames: usize,
    min_speech_frames: usize,
    padding_frames: usize,
    /// Samples waiting for a full VAD frame.
    pending: Vec<f32>,
    /// Audio of the current chunk, always a whole number of frames.
    audio: Vec<f32>,
    /// One speech flag per frame in `audio`.
    flags: Vec<bool>,
    speech_frames: usize,
    trailing_silence: usize,
    frames_since_speech: usize,
}

impl<V: Vad> Chunker<V> {
    pub fn new(vad: V, config: Config) -> Self {
        Self {
            vad,
            pause_frames: frames_in(config.pause_ms),
            max_frames: config.max_chunk_ms / FRAME_MS,
            soft_pause_frames: frames_in(config.soft_pause_ms),
            min_speech_frames: frames_in(config.min_speech_ms),
            padding_frames: frames_in(config.padding_ms),
            pending: Vec::with_capacity(FRAME),
            audio: Vec::new(),
            flags: Vec::new(),
            speech_frames: 0,
            trailing_silence: 0,
            frames_since_speech: 0,
        }
    }

    /// Feeds audio of any length and returns the chunks completed by it.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut chunks = Vec::new();
        self.pending.extend_from_slice(samples);
        let whole = self.pending.len() / FRAME * FRAME;
        let ready: Vec<f32> = self.pending.drain(..whole).collect();
        for frame in ready.as_chunks::<FRAME>().0 {
            chunks.extend(self.push_frame(frame));
        }
        chunks
    }

    fn push_frame(&mut self, frame: &[f32]) -> Option<Vec<f32>> {
        let is_speech = self.vad.is_speech(frame);
        self.frames_since_speech = if is_speech {
            0
        } else {
            self.frames_since_speech + 1
        };
        self.audio.extend_from_slice(frame);
        self.flags.push(is_speech);

        if is_speech {
            self.speech_frames += 1;
            self.trailing_silence = 0;
        } else if self.speech_frames == 0 {
            self.trim_leading_silence();
            return None;
        } else {
            self.trailing_silence += 1;
        }

        if self.trailing_silence >= self.pause_frames {
            let speech_end = self.flags.len() - self.trailing_silence;
            self.cut((speech_end + self.padding_frames).min(self.flags.len()))
        } else if self.flags.len() >= self.max_frames {
            self.cut(self.best_cut())
        } else {
            None
        }
    }

    /// Middle of the latest pause long enough to cut at, or the very end if there is none.
    fn best_cut(&self) -> usize {
        let mut run = 0;
        for (index, is_speech) in self.flags.iter().enumerate().rev() {
            if *is_speech {
                if run >= self.soft_pause_frames {
                    return index + 1 + run / 2;
                }
                run = 0;
            } else {
                run += 1;
            }
        }
        self.flags.len()
    }

    /// Takes the first `frames` frames as a chunk, keeping the rest for the next one.
    fn cut(&mut self, frames: usize) -> Option<Vec<f32>> {
        let chunk_speech = self.flags[..frames].iter().filter(|f| **f).count();
        let chunk: Vec<f32> = self.audio.drain(..frames * FRAME).collect();
        self.flags.drain(..frames);

        self.speech_frames = self.flags.iter().filter(|f| **f).count();
        self.trailing_silence = self.flags.iter().rev().take_while(|f| !**f).count();
        if self.speech_frames == 0 {
            self.trailing_silence = 0;
            self.trim_leading_silence();
        }
        (chunk_speech >= self.min_speech_frames).then_some(chunk)
    }

    fn trim_leading_silence(&mut self) {
        if self.flags.len() > self.padding_frames {
            let extra = self.flags.len() - self.padding_frames;
            self.flags.drain(..extra);
            self.audio.drain(..extra * FRAME);
        }
    }

    /// Returns what is left at the end of the recording, if it contains speech.
    pub fn flush(&mut self) -> Option<Vec<f32>> {
        self.pending.clear();
        if self.speech_frames == 0 {
            return None;
        }
        let speech_end = self.flags.len() - self.trailing_silence;
        let end = (speech_end + self.padding_frames).min(self.flags.len());
        let chunk = self.cut(end);
        self.audio.clear();
        self.flags.clear();
        self.speech_frames = 0;
        self.trailing_silence = 0;
        chunk
    }

    /// Time since speech was last heard, or since the start if there was none.
    pub fn silence_ms(&self) -> usize {
        self.frames_since_speech * FRAME_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Treats any frame with a loud sample as speech.
    struct LoudVad;
    impl Vad for LoudVad {
        fn is_speech(&mut self, frame: &[f32]) -> bool {
            frame.iter().any(|s| s.abs() > 0.5)
        }
    }

    fn silence(ms: usize) -> Vec<f32> {
        vec![0.0; ms * SAMPLE_RATE / 1000]
    }

    fn speech(ms: usize) -> Vec<f32> {
        vec![0.9; ms * SAMPLE_RATE / 1000]
    }

    fn ms_of(samples: &[f32]) -> usize {
        samples.len() * 1000 / SAMPLE_RATE
    }

    fn chunker() -> Chunker<LoudVad> {
        Chunker::new(LoudVad, Config::default())
    }

    #[test]
    fn silence_produces_no_chunks() {
        let mut c = chunker();
        assert!(c.push(&silence(5_000)).is_empty());
        assert_eq!(c.flush(), None);
    }

    #[test]
    fn speech_then_pause_produces_one_chunk() {
        let mut c = chunker();
        let chunks = c.push(&[speech(1_000), silence(800)].concat());
        assert_eq!(chunks.len(), 1);
        assert_eq!(c.flush(), None);
    }

    #[test]
    fn a_pause_shorter_than_the_limit_does_not_cut() {
        let mut c = chunker();
        let chunks = c.push(&[speech(1_000), silence(400), speech(1_000)].concat());
        assert!(chunks.is_empty());
        assert!(c.flush().is_some());
    }

    #[test]
    fn two_sentences_produce_two_chunks_in_order() {
        let mut c = chunker();
        let first = [vec![0.9; SAMPLE_RATE], silence(800)].concat();
        let second = [vec![-0.8; SAMPLE_RATE], silence(800)].concat();
        let chunks = c.push(&[first, second].concat());
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains(&0.9) && !chunks[0].contains(&-0.8));
        assert!(chunks[1].contains(&-0.8) && !chunks[1].contains(&0.9));
    }

    #[test]
    fn chunk_has_padding_on_both_sides_and_no_long_silence() {
        let mut c = chunker();
        let chunks = c.push(&[silence(1_000), speech(1_000), silence(1_000)].concat());
        assert_eq!(chunks.len(), 1);
        let ms = ms_of(&chunks[0]);
        assert!((1_390..=1_450).contains(&ms), "chunk was {ms} ms");
    }

    #[test]
    fn continuous_speech_is_cut_before_the_limit() {
        let mut c = chunker();
        let chunks = c.push(&speech(60_000));
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(ms_of(chunk) <= 25_500, "chunk was {} ms", ms_of(chunk));
        }
    }

    #[test]
    fn no_audio_is_lost_when_a_long_chunk_is_force_cut() {
        let mut c = chunker();
        let mut chunks = c.push(&speech(60_000));
        chunks.extend(c.flush());
        let total: usize = chunks.iter().map(|chunk| ms_of(chunk)).sum();
        assert!(total >= 59_900, "total was {total} ms");
    }

    #[test]
    fn long_chunk_prefers_a_short_pause_as_the_cut_point() {
        let mut c = chunker();
        let audio = [speech(20_000), silence(300), speech(10_000)].concat();
        let chunks = c.push(&audio);
        assert_eq!(chunks.len(), 1);
        let ms = ms_of(&chunks[0]);
        assert!((20_000..=20_400).contains(&ms), "chunk was {ms} ms");
        let rest = c.flush().expect("speech after the cut must be kept");
        let rest_ms = ms_of(&rest);
        assert!((9_900..=10_400).contains(&rest_ms), "rest was {rest_ms} ms");
    }

    #[test]
    fn too_little_speech_is_dropped() {
        let mut c = chunker();
        let chunks = c.push(&[silence(500), speech(100), silence(1_000)].concat());
        assert!(chunks.is_empty());
        assert_eq!(c.flush(), None);
    }

    #[test]
    fn flush_returns_remaining_speech_and_empties_the_buffer() {
        let mut c = chunker();
        assert!(c.push(&speech(1_000)).is_empty());
        assert!(c.flush().is_some());
        assert_eq!(c.flush(), None);
    }

    #[test]
    fn input_of_any_size_gives_the_same_result() {
        let audio = [speech(1_000), silence(800), speech(1_000), silence(800)].concat();
        let mut whole = chunker();
        let expected = whole.push(&audio);

        let mut tiny = chunker();
        let mut got = Vec::new();
        for piece in audio.chunks(480) {
            got.extend(tiny.push(piece));
        }
        assert_eq!(got, expected);
    }

    #[test]
    fn silence_timer_counts_from_the_start_and_resets_on_speech() {
        let mut c = chunker();
        c.push(&silence(1_000));
        assert!(
            (980..=1_000).contains(&c.silence_ms()),
            "{}",
            c.silence_ms()
        );
        c.push(&speech(500));
        assert_eq!(c.silence_ms(), 0);
        c.push(&silence(2_000));
        assert!(
            (1_980..=2_000).contains(&c.silence_ms()),
            "{}",
            c.silence_ms()
        );
    }
}
