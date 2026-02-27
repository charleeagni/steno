//! Voice Activity Detection iterator implementation
//! 
//! This module provides the VAD iterator for processing audio streams and detecting speech segments.
//! It handles both streaming and batch processing of audio data.

use crate::{Result, SileroVAD};
use ndarray::{ArrayView1, Array2};
use serde::{Deserialize, Serialize};
use log::debug;

/// Speech timestamp information
/// 
/// Represents a segment of speech detected in the audio stream.
/// Times are in seconds from the start of the audio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechTimestamps {
    /// Start time in seconds
    pub start: f32,
    /// End time in seconds
    pub end: f32,
}

/// Iterator for processing audio in chunks
/// 
/// This struct provides a convenient interface for processing audio streams
/// and detecting speech segments. It maintains internal state to handle
/// streaming audio and can be used for both real-time and batch processing.
/// 
/// # Example
/// 
/// ```rust
/// use silero_vad::{SileroVAD, VADIterator};
/// use ndarray::Array1;
/// 
/// let model = SileroVAD::new("path/to/model.onnx")?;
/// let mut vad = VADIterator::new(
///     model,
///     0.5,  // threshold
///     16000, // sampling rate
///     100,   // min silence duration (ms)
///     30,    // speech pad (ms)
/// );
/// 
/// let audio_chunk = Array1::zeros(512);
/// if let Some(ts) = vad.process_chunk(&audio_chunk.view())? {
///     println!("Speech detected from {:.2}s to {:.2}s", ts.start, ts.end);
/// }
/// ```
pub struct VADIterator {
    model: SileroVAD,
    threshold: f32,
    sampling_rate: u32,
    min_silence_duration_ms: u32,
    speech_pad_ms: u32,
    speech_start: Option<f32>,
    speech_end: Option<f32>,
    last_prob: f32,
}

impl VADIterator {
    /// Create a new VAD iterator
    /// 
    /// # Arguments
    /// 
    /// * `model` - The Silero VAD model to use
    /// * `threshold` - Speech detection threshold (0.0 to 1.0)
    /// * `sampling_rate` - Audio sampling rate (must be 16kHz)
    /// * `min_silence_duration_ms` - Minimum silence duration to end speech segment
    /// * `speech_pad_ms` - Padding to add to speech segments
    pub fn new(
        model: SileroVAD,
        threshold: f32,
        sampling_rate: u32,
        min_silence_duration_ms: u32,
        speech_pad_ms: u32,
    ) -> Self {
        Self {
            model,
            threshold,
            sampling_rate,
            min_silence_duration_ms,
            speech_pad_ms,
            speech_start: None,
            speech_end: None,
            last_prob: 0.0,
        }
    }

    /// Reset the iterator state
    /// 
    /// This should be called when processing a new audio stream or when
    /// you want to clear the internal state.
    pub fn reset(&mut self) {
        self.speech_start = None;
        self.speech_end = None;
        self.last_prob = 0.0;
        self.model.reset_states(1);
    }

    /// Process a single audio chunk and return speech timestamps if detected
    /// 
    /// # Arguments
    /// 
    /// * `x` - Audio chunk to process (must be 512 samples for 16kHz)
    /// 
    /// # Returns
    /// 
    /// `Ok(Some(timestamps))` if speech is detected, `Ok(None)` otherwise
    /// 
    /// # Errors
    /// 
    /// Returns an error if:
    /// * The input chunk size is invalid
    /// * Model inference fails
    pub fn process_chunk(&mut self, x: &ArrayView1<f32>) -> Result<Option<SpeechTimestamps>> {
        let prob = self.model.process_chunk(x, self.sampling_rate)?;
        let prob = prob[0];

        let mut result = None;
        let time_per_sample = 1.0 / self.sampling_rate as f32;
        let current_time = (x.len() as f32) * time_per_sample;

        if prob >= self.threshold {
            if self.speech_start.is_none() {
                self.speech_start = Some(current_time);
            }
            self.speech_end = Some(current_time);
        } else if self.speech_start.is_some() {
            let silence_duration = current_time - self.speech_end.unwrap();
            let silence_duration_ms = (silence_duration * 1000.0) as u32;

            if silence_duration_ms >= self.min_silence_duration_ms {
                let start = self.speech_start.unwrap();
                let end = self.speech_end.unwrap() + (self.speech_pad_ms as f32 / 1000.0);
                result = Some(SpeechTimestamps { start, end });
                self.reset();
            }
        }

        self.last_prob = prob;
        Ok(result)
    }

    /// Get speech timestamps for an entire audio file
    /// 
    /// # Arguments
    /// 
    /// * `audio` - Complete audio file to process
    /// * `min_speech_duration_ms` - Minimum duration of speech segments
    /// * `max_speech_duration_s` - Maximum duration of speech segments
    /// * `min_silence_duration_ms` - Minimum silence duration between segments
    /// * `speech_pad_ms` - Padding to add to speech segments
    /// 
    /// # Returns
    /// 
    /// Vector of speech timestamps for all detected segments
    /// 
    /// # Errors
    /// 
    /// Returns an error if:
    /// * The audio data is invalid
    /// * Model inference fails
    pub fn get_speech_timestamps(
        &mut self,
        audio: &ArrayView1<f32>,
        min_speech_duration_ms: u32,
        max_speech_duration_s: f32,
        _min_silence_duration_ms: u32,
        _speech_pad_ms: u32,
    ) -> Result<Vec<SpeechTimestamps>> {
        let mut timestamps = Vec::new();
        let chunk_size = if self.sampling_rate == 16000 { 512 } else { 256 };
        
        // Process audio chunks one at a time
        let mut i = 0;
        while i < audio.len() {
            let end = (i + chunk_size).min(audio.len());
            if end - i < chunk_size {
                break;
            }

            debug!("Processing chunk at position {}", i);
            
            // Process the chunk
            let window = audio.slice(ndarray::s![i..end]);
            if let Some(ts) = self.process_chunk(&window)? {
                if ts.end - ts.start >= min_speech_duration_ms as f32 / 1000.0
                    && ts.end - ts.start <= max_speech_duration_s
                {
                    timestamps.push(ts);
                }
            }

            i = end;
        }

        Ok(timestamps)
    }

    /// Process a batch of audio chunks and return speech timestamps if detected
    /// 
    /// # Arguments
    /// 
    /// * `x` - Batch of audio chunks to process (each chunk must be 512 samples for 16kHz)
    /// 
    /// # Returns
    /// 
    /// `Ok(Some(timestamps))` if speech is detected, `Ok(None)` otherwise
    /// 
    /// # Errors
    /// 
    /// Returns an error if:
    /// * The input chunk size is invalid
    /// * Model inference fails
    pub fn process_batch(&mut self, x: &Array2<f32>) -> Result<Option<Vec<SpeechTimestamps>>> {
        let probs = self.model.process_batch(x, self.sampling_rate)?;
        let mut results = Vec::new();
        let time_per_sample = 1.0 / self.sampling_rate as f32;
        let chunk_duration = (x.ncols() as f32) * time_per_sample;

        for (i, &prob) in probs.iter().enumerate() {
            let current_time = (i as f32 + 1.0) * chunk_duration;

            if prob >= self.threshold {
                if self.speech_start.is_none() {
                    self.speech_start = Some(current_time - chunk_duration);
                }
                self.speech_end = Some(current_time);
            } else if self.speech_start.is_some() {
                let silence_duration = current_time - self.speech_end.unwrap();
                let silence_duration_ms = (silence_duration * 1000.0) as u32;

                if silence_duration_ms >= self.min_silence_duration_ms {
                    let start = self.speech_start.unwrap();
                    let end = self.speech_end.unwrap() + (self.speech_pad_ms as f32 / 1000.0);
                    results.push(SpeechTimestamps { start, end });
                    self.reset();
                }
            }

            self.last_prob = prob;
        }

        Ok(if results.is_empty() { None } else { Some(results) })
    }
} 