//! Audio utilities for the Silero VAD library
//! 
//! This module provides utility functions for reading and writing audio files,
//! as well as processing audio chunks based on speech timestamps.

use crate::{Error, Result};
use ndarray::{Array1, s};
use std::path::Path;

/// Read audio from a WAV file
/// 
/// # Arguments
/// 
/// * `path` - Path to the WAV file
/// * `sampling_rate` - Expected sampling rate of the audio
/// 
/// # Returns
/// 
/// Audio data as a 1D array of f32 samples
/// 
/// # Errors
/// 
/// Returns an error if:
/// * The file cannot be opened
/// * The file format is invalid
/// * The sampling rate doesn't match
/// * The audio data cannot be read
pub fn read_audio<P: AsRef<Path>>(path: P, sampling_rate: u32) -> Result<Array1<f32>> {
    let mut reader = hound::WavReader::open(path).map_err(|e| Error::AudioProcessing(e.to_string()))?;
    
    if reader.spec().sample_rate != sampling_rate {
        return Err(Error::AudioProcessing(format!(
            "Audio file has sampling rate {}, but {} was requested",
            reader.spec().sample_rate,
            sampling_rate
        )));
    }

    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.map_err(|e| Error::AudioProcessing(e.to_string())))
        .map(|s| s.map(|v| v as f32 / 32768.0))
        .collect::<Result<Vec<f32>>>()?;

    Ok(Array1::from_vec(samples))
}

/// Save audio to a WAV file
/// 
/// # Arguments
/// 
/// * `path` - Path to save the WAV file
/// * `audio` - Audio data as a 1D array of f32 samples
/// * `sampling_rate` - Sampling rate of the audio
/// 
/// # Errors
/// 
/// Returns an error if:
/// * The file cannot be created
/// * The audio data cannot be written
/// * The WAV file cannot be finalized
pub fn save_audio<P: AsRef<Path>>(path: P, audio: &Array1<f32>, sampling_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sampling_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| Error::AudioProcessing(e.to_string()))?;

    for &sample in audio.iter() {
        let sample = (sample * 32768.0).max(-32768.0).min(32767.0) as i16;
        writer
            .write_sample(sample)
            .map_err(|e| Error::AudioProcessing(e.to_string()))?;
    }

    writer
        .finalize()
        .map_err(|e| Error::AudioProcessing(e.to_string()))?;

    Ok(())
}

/// Collect audio chunks based on speech timestamps
/// 
/// This function extracts audio segments corresponding to speech timestamps
/// and concatenates them into a single audio array.
/// 
/// # Arguments
/// 
/// * `timestamps` - Speech timestamps to extract
/// * `audio` - Complete audio data
/// * `sampling_rate` - Sampling rate of the audio
/// 
/// # Returns
/// 
/// Concatenated audio segments as a 1D array
/// 
/// # Errors
/// 
/// Returns an error if:
/// * Any timestamp is out of bounds
/// * The audio data is invalid
pub fn collect_chunks(
    timestamps: &[crate::vad::SpeechTimestamps],
    audio: &Array1<f32>,
    sampling_rate: u32,
) -> Result<Array1<f32>> {
    let mut result = Vec::new();
    let samples_per_second = sampling_rate as usize;

    for ts in timestamps {
        let start_sample = (ts.start * samples_per_second as f32) as usize;
        let end_sample = (ts.end * samples_per_second as f32) as usize;
        
        if start_sample >= audio.len() || end_sample > audio.len() {
            return Err(Error::InvalidInput(format!(
                "Timestamp out of bounds: {} - {} (audio length: {})",
                start_sample,
                end_sample,
                audio.len()
            )));
        }

        result.extend_from_slice(&audio.slice(s![start_sample..end_sample]).to_vec());
    }

    Ok(Array1::from_vec(result))
}

/// Drop audio chunks based on speech timestamps
/// 
/// This function removes audio segments corresponding to speech timestamps
/// and concatenates the remaining segments.
/// 
/// # Arguments
/// 
/// * `timestamps` - Speech timestamps to remove
/// * `audio` - Complete audio data
/// * `sampling_rate` - Sampling rate of the audio
/// 
/// # Returns
/// 
/// Audio with speech segments removed as a 1D array
/// 
/// # Errors
/// 
/// Returns an error if:
/// * Any timestamp is out of bounds
/// * The audio data is invalid
pub fn drop_chunks(
    timestamps: &[crate::vad::SpeechTimestamps],
    audio: &Array1<f32>,
    sampling_rate: u32,
) -> Result<Array1<f32>> {
    let mut result = Vec::new();
    let samples_per_second = sampling_rate as usize;
    let mut current_pos = 0;

    for ts in timestamps {
        let start_sample = (ts.start * samples_per_second as f32) as usize;
        let end_sample = (ts.end * samples_per_second as f32) as usize;
        
        if start_sample >= audio.len() || end_sample > audio.len() {
            return Err(Error::InvalidInput(format!(
                "Timestamp out of bounds: {} - {} (audio length: {})",
                start_sample,
                end_sample,
                audio.len()
            )));
        }

        if start_sample > current_pos {
            result.extend_from_slice(&audio.slice(s![current_pos..start_sample]).to_vec());
        }
        current_pos = end_sample;
    }

    if current_pos < audio.len() {
        result.extend_from_slice(&audio.slice(s![current_pos..]).to_vec());
    }

    Ok(Array1::from_vec(result))
} 