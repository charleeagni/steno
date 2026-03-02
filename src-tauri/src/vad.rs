use thiserror::Error;
use webrtc_vad::{SampleRate, Vad, VadMode};

pub const LIVE_WORD_VAD_SAMPLE_RATE: u32 = 16_000;
pub const LIVE_WORD_VAD_MIN_SILENCE_MS: u64 = 90;
pub const LIVE_WORD_VAD_SPEECH_PAD_MS: u64 = 30;
pub const LIVE_WORD_VAD_MIN_SPEECH_MS: u64 = 40;
pub const LIVE_WORD_VAD_CHUNK_SIZE: usize = 160;
pub const LIVE_WORD_VAD_MAX_WORD_MS: u64 = 1200;

#[derive(Debug, Error)]
pub enum LiveWordVadError {
    #[error("Invalid VAD frame: {0}")]
    InvalidFrame(String),
}

pub struct LiveWordVad {
    pending_samples: Vec<f32>,
    processed_samples: usize,
    in_speech: bool,
    speech_start_sample: usize,
    speech_run_samples: usize,
    silence_run_samples: usize,
}

impl LiveWordVad {
    pub fn new() -> Result<Self, LiveWordVadError> {
        Ok(Self {
            pending_samples: Vec::new(),
            processed_samples: 0,
            in_speech: false,
            speech_start_sample: 0,
            speech_run_samples: 0,
            silence_run_samples: 0,
        })
    }

    pub fn reset(&mut self) {
        // Keep detector and clear segment state.

        self.pending_samples.clear();
        self.processed_samples = 0;
        self.in_speech = false;
        self.speech_start_sample = 0;
        self.speech_run_samples = 0;
        self.silence_run_samples = 0;
    }

    pub fn push_samples(
        &mut self,
        samples: &[f32],
    ) -> Result<Vec<(usize, usize)>, LiveWordVadError> {
        // Build detector per call to keep state Send-safe.

        let mut detector = build_detector();

        if samples.is_empty() {
            return Ok(Vec::new());
        }

        self.pending_samples.extend_from_slice(samples);

        let min_speech_samples =
            ms_to_samples(LIVE_WORD_VAD_SAMPLE_RATE, LIVE_WORD_VAD_MIN_SPEECH_MS);
        let min_silence_samples =
            ms_to_samples(LIVE_WORD_VAD_SAMPLE_RATE, LIVE_WORD_VAD_MIN_SILENCE_MS);
        let speech_pad_samples =
            ms_to_samples(LIVE_WORD_VAD_SAMPLE_RATE, LIVE_WORD_VAD_SPEECH_PAD_MS);
        let max_word_samples = ms_to_samples(LIVE_WORD_VAD_SAMPLE_RATE, LIVE_WORD_VAD_MAX_WORD_MS);

        let mut consumed = 0usize;
        let mut segments = Vec::new();
        while self.pending_samples.len().saturating_sub(consumed) >= LIVE_WORD_VAD_CHUNK_SIZE {
            let chunk_end = consumed + LIVE_WORD_VAD_CHUNK_SIZE;
            let frame = f32_frame_to_i16(&self.pending_samples[consumed..chunk_end]);
            let is_speech = detector
                .is_voice_segment(&frame)
                .map_err(|_| LiveWordVadError::InvalidFrame("invalid frame size".to_string()))?;

            self.processed_samples += LIVE_WORD_VAD_CHUNK_SIZE;
            let global_end = self.processed_samples;

            if self.in_speech {
                if is_speech {
                    self.silence_run_samples = 0;
                } else {
                    self.silence_run_samples += LIVE_WORD_VAD_CHUNK_SIZE;
                }

                let speech_span = global_end.saturating_sub(self.speech_start_sample);
                let reached_silence_boundary = self.silence_run_samples >= min_silence_samples;
                let reached_max_duration = speech_span >= max_word_samples;

                if reached_silence_boundary || reached_max_duration {
                    let speech_end = if reached_silence_boundary {
                        global_end.saturating_sub(self.silence_run_samples)
                    } else {
                        global_end
                    };

                    if speech_end > self.speech_start_sample
                        && speech_end - self.speech_start_sample >= min_speech_samples
                    {
                        let start = self.speech_start_sample.saturating_sub(speech_pad_samples);
                        let end = speech_end.saturating_add(speech_pad_samples);
                        segments.push((start, end));
                    }

                    self.in_speech = false;
                    self.speech_run_samples = 0;
                    self.silence_run_samples = 0;
                }

                consumed += LIVE_WORD_VAD_CHUNK_SIZE;
                continue;
            }

            if is_speech {
                self.speech_run_samples += LIVE_WORD_VAD_CHUNK_SIZE;
            } else {
                self.speech_run_samples = 0;
            }

            if self.speech_run_samples >= min_speech_samples {
                self.in_speech = true;
                self.speech_start_sample = global_end.saturating_sub(self.speech_run_samples);
                self.speech_run_samples = 0;
                self.silence_run_samples = 0;
            }

            consumed += LIVE_WORD_VAD_CHUNK_SIZE;
        }

        if consumed > 0 {
            self.pending_samples.drain(..consumed);
        }

        Ok(segments)
    }
}

fn ms_to_samples(sample_rate: u32, ms: u64) -> usize {
    ((sample_rate as u64 * ms) / 1000) as usize
}

fn f32_frame_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| {
            let clamped = sample.clamp(-1.0, 1.0);
            (clamped * i16::MAX as f32) as i16
        })
        .collect()
}

fn build_detector() -> Vad {
    // Use moderate aggression for balanced speech filtering.

    Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Quality)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms_to_samples_converts_16khz_values() {
        assert_eq!(ms_to_samples(16_000, 30), 480);
        assert_eq!(ms_to_samples(16_000, 40), 640);
        assert_eq!(ms_to_samples(16_000, 90), 1440);
    }

    #[test]
    fn live_word_chunk_size_matches_10ms_frame_at_16khz() {
        assert_eq!(LIVE_WORD_VAD_CHUNK_SIZE, 160);
    }
}
