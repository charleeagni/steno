use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub struct CapturedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureLifecycleState {
    Idle,
    Recording,
    Stopping,
    Finalizing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CaptureDestinationPolicy {
    Temp,
    AppData,
    CustomPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingArtifactPaths {
    pub recording_wav_path: PathBuf,
    pub transcript_output_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum CaptureEngineError {
    #[error("Invalid capture lifecycle transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: CaptureLifecycleState,
        to: CaptureLifecycleState,
    },
    #[error("No default input device is available")]
    MissingInputDevice,
    #[error("Failed to read default input stream config: {0}")]
    InputConfigRead(String),
    #[error("Failed to build input stream: {0}")]
    InputStreamBuild(String),
    #[error("Unsupported input sample format for recorder: {0:?}")]
    UnsupportedSampleFormat(SampleFormat),
    #[error("Failed to start microphone stream: {0}")]
    StreamStart(String),
    #[error("Audio sample sink lock poisoned")]
    SinkLockPoisoned,
    #[error("custom_path destination policy requires custom path")]
    MissingCustomPath,
    #[error("app_data destination policy requires app data path")]
    MissingAppDataPath,
    #[error("Failed to create destination directory at {path}: {reason}")]
    DestinationCreateFailed { path: PathBuf, reason: String },
    #[error("Destination path is not writable at {path}: {reason}")]
    DestinationNotWritable { path: PathBuf, reason: String },
    #[error("Failed to create WAV file at {path}: {reason}")]
    WavCreateFailed { path: PathBuf, reason: String },
    #[error("Failed to write WAV sample: {0}")]
    WavSampleWriteFailed(String),
    #[error("Failed to finalize WAV file: {0}")]
    WavFinalizeFailed(String),
}

pub struct AudioCapture {
    stream: Option<Stream>,
    sink: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    state: CaptureLifecycleState,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            sink: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 16_000,
            state: CaptureLifecycleState::Idle,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.state == CaptureLifecycleState::Recording && self.stream.is_some()
    }

    pub fn start(&mut self) -> std::result::Result<(), CaptureEngineError> {
        let mut next_state = self.state;
        transition_state(&mut next_state, CaptureLifecycleState::Recording)?;

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(CaptureEngineError::MissingInputDevice)
            .inspect_err(|_| {
                self.state = CaptureLifecycleState::Failed;
            })?;

        let default_config = device
            .default_input_config()
            .map_err(|err| CaptureEngineError::InputConfigRead(err.to_string()))
            .inspect_err(|_| {
                self.state = CaptureLifecycleState::Failed;
            })?;
        let sample_format = default_config.sample_format();
        let config: cpal::StreamConfig = default_config.clone().into();
        let channels = usize::from(config.channels);

        let sink = Arc::new(Mutex::new(Vec::new()));
        self.sink = sink.clone();
        self.sample_rate = config.sample_rate.0;

        let err_fn = |err| {
            log::error!("event=audio_stream_error error={}", err);
        };

        let stream_result: std::result::Result<Stream, CaptureEngineError> = match sample_format {
            SampleFormat::F32 => {
                let sink = sink.clone();
                device
                    .build_input_stream(
                        &config,
                        move |data: &[f32], _| {
                            push_f32_mono(data, channels, &sink);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| CaptureEngineError::InputStreamBuild(err.to_string()))
            }
            SampleFormat::I16 => {
                let sink = sink.clone();
                device
                    .build_input_stream(
                        &config,
                        move |data: &[i16], _| {
                            push_i16_mono(data, channels, &sink);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| CaptureEngineError::InputStreamBuild(err.to_string()))
            }
            SampleFormat::U16 => {
                let sink = sink.clone();
                device
                    .build_input_stream(
                        &config,
                        move |data: &[u16], _| {
                            push_u16_mono(data, channels, &sink);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| CaptureEngineError::InputStreamBuild(err.to_string()))
            }
            other => Err(CaptureEngineError::UnsupportedSampleFormat(other)),
        };
        let stream = stream_result.inspect_err(|_| {
            self.state = CaptureLifecycleState::Failed;
        })?;

        if let Err(err) = stream.play() {
            self.state = CaptureLifecycleState::Failed;
            return Err(CaptureEngineError::StreamStart(err.to_string()));
        }
        self.stream = Some(stream);
        self.state = next_state;

        Ok(())
    }

    pub fn stop(&mut self) -> std::result::Result<CapturedAudio, CaptureEngineError> {
        transition_state(&mut self.state, CaptureLifecycleState::Stopping)?;
        let Some(stream) = self.stream.take() else {
            self.state = CaptureLifecycleState::Failed;
            return Err(CaptureEngineError::InvalidTransition {
                from: CaptureLifecycleState::Stopping,
                to: CaptureLifecycleState::Completed,
            });
        };

        drop(stream);

        let mut guard = self.sink.lock().map_err(|_| {
            self.state = CaptureLifecycleState::Failed;
            CaptureEngineError::SinkLockPoisoned
        })?;
        let samples = std::mem::take(&mut *guard);
        self.state = CaptureLifecycleState::Completed;

        Ok(CapturedAudio {
            samples,
            sample_rate: self.sample_rate,
        })
    }

    pub fn snapshot(&self) -> std::result::Result<CapturedAudio, CaptureEngineError> {
        if self.state != CaptureLifecycleState::Recording || self.stream.is_none() {
            return Err(CaptureEngineError::InvalidTransition {
                from: self.state,
                to: CaptureLifecycleState::Recording,
            });
        }

        let guard = self
            .sink
            .lock()
            .map_err(|_| CaptureEngineError::SinkLockPoisoned)?;

        Ok(CapturedAudio {
            samples: guard.clone(),
            sample_rate: self.sample_rate,
        })
    }
}

fn push_f32_mono(data: &[f32], channels: usize, sink: &Arc<Mutex<Vec<f32>>>) {
    if channels == 0 {
        return;
    }

    if let Ok(mut guard) = sink.lock() {
        for frame in data.chunks(channels) {
            let avg = frame.iter().copied().sum::<f32>() / frame.len() as f32;
            guard.push(avg);
        }
    }
}

fn push_i16_mono(data: &[i16], channels: usize, sink: &Arc<Mutex<Vec<f32>>>) {
    if channels == 0 {
        return;
    }

    if let Ok(mut guard) = sink.lock() {
        for frame in data.chunks(channels) {
            let mut sum = 0.0_f32;
            for sample in frame {
                sum += *sample as f32 / i16::MAX as f32;
            }
            guard.push(sum / frame.len() as f32);
        }
    }
}

fn push_u16_mono(data: &[u16], channels: usize, sink: &Arc<Mutex<Vec<f32>>>) {
    if channels == 0 {
        return;
    }

    if let Ok(mut guard) = sink.lock() {
        for frame in data.chunks(channels) {
            let mut sum = 0.0_f32;
            for sample in frame {
                let centered = (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0;
                sum += centered;
            }
            guard.push(sum / frame.len() as f32);
        }
    }
}

pub fn finalize_recording_wav(
    audio: &CapturedAudio,
    session_id: &str,
    destination_policy: CaptureDestinationPolicy,
    app_data_path: Option<&Path>,
    custom_output_path: Option<&Path>,
) -> std::result::Result<RecordingArtifactPaths, CaptureEngineError> {
    let mut state = CaptureLifecycleState::Stopping;
    transition_state(&mut state, CaptureLifecycleState::Finalizing)?;

    let artifacts = build_recording_artifact_paths(
        session_id,
        destination_policy,
        app_data_path,
        custom_output_path,
    )?;

    if let Err(err) = write_wav_file_typed(&artifacts.recording_wav_path, audio) {
        transition_state(&mut state, CaptureLifecycleState::Failed)?;
        return Err(err);
    }

    transition_state(&mut state, CaptureLifecycleState::Completed)?;
    Ok(artifacts)
}

pub fn build_recording_artifact_paths(
    session_id: &str,
    destination_policy: CaptureDestinationPolicy,
    app_data_path: Option<&Path>,
    custom_output_path: Option<&Path>,
) -> std::result::Result<RecordingArtifactPaths, CaptureEngineError> {
    let destination_dir =
        resolve_destination_dir(destination_policy, app_data_path, custom_output_path)?;

    Ok(RecordingArtifactPaths {
        recording_wav_path: destination_dir.join(format!("recording_{}.wav", session_id)),
        transcript_output_path: destination_dir.join(format!("transcript_{}.txt", session_id)),
    })
}

fn write_wav_file_typed(
    path: &Path,
    audio: &CapturedAudio,
) -> std::result::Result<(), CaptureEngineError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: WavSampleFormat::Int,
    };

    let mut writer =
        WavWriter::create(path, spec).map_err(|err| CaptureEngineError::WavCreateFailed {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;

    for sample in &audio.samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_sample = (clamped * i16::MAX as f32) as i16;
        writer
            .write_sample(i16_sample)
            .map_err(|err| CaptureEngineError::WavSampleWriteFailed(err.to_string()))?;
    }

    writer
        .finalize()
        .map_err(|err| CaptureEngineError::WavFinalizeFailed(err.to_string()))?;
    Ok(())
}

fn resolve_destination_dir(
    destination_policy: CaptureDestinationPolicy,
    app_data_path: Option<&Path>,
    custom_output_path: Option<&Path>,
) -> std::result::Result<PathBuf, CaptureEngineError> {
    let dir = match destination_policy {
        CaptureDestinationPolicy::Temp => std::env::temp_dir().join("recorder-capture"),
        CaptureDestinationPolicy::AppData => app_data_path
            .map(Path::to_path_buf)
            .ok_or(CaptureEngineError::MissingAppDataPath)?,
        CaptureDestinationPolicy::CustomPath => custom_output_path
            .map(Path::to_path_buf)
            .ok_or(CaptureEngineError::MissingCustomPath)?,
    };

    fs::create_dir_all(&dir).map_err(|err| CaptureEngineError::DestinationCreateFailed {
        path: dir.clone(),
        reason: err.to_string(),
    })?;
    validate_destination_writable(&dir)?;
    Ok(dir)
}

fn validate_destination_writable(path: &Path) -> std::result::Result<(), CaptureEngineError> {
    let probe_name = format!(
        ".write_probe_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let probe_path = path.join(probe_name);
    let mut open_options = OpenOptions::new();
    open_options.create_new(true).write(true);
    open_options
        .open(&probe_path)
        .map_err(|err| CaptureEngineError::DestinationNotWritable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
    let _ = fs::remove_file(probe_path);
    Ok(())
}

fn transition_state(
    state: &mut CaptureLifecycleState,
    target: CaptureLifecycleState,
) -> std::result::Result<(), CaptureEngineError> {
    if is_valid_transition(*state, target) {
        *state = target;
        Ok(())
    } else {
        Err(CaptureEngineError::InvalidTransition {
            from: *state,
            to: target,
        })
    }
}

fn is_valid_transition(from: CaptureLifecycleState, to: CaptureLifecycleState) -> bool {
    matches!(
        (from, to),
        (
            CaptureLifecycleState::Idle,
            CaptureLifecycleState::Recording
        ) | (
            CaptureLifecycleState::Recording,
            CaptureLifecycleState::Stopping
        ) | (
            CaptureLifecycleState::Stopping,
            CaptureLifecycleState::Finalizing
        ) | (
            CaptureLifecycleState::Stopping,
            CaptureLifecycleState::Completed
        ) | (
            CaptureLifecycleState::Finalizing,
            CaptureLifecycleState::Completed
        ) | (
            CaptureLifecycleState::Finalizing,
            CaptureLifecycleState::Failed
        ) | (
            CaptureLifecycleState::Completed,
            CaptureLifecycleState::Recording
        ) | (
            CaptureLifecycleState::Failed,
            CaptureLifecycleState::Recording
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_rejects_invalid_transition() {
        let mut state = CaptureLifecycleState::Idle;
        let result = transition_state(&mut state, CaptureLifecycleState::Finalizing);
        assert!(result.is_err());
        assert_eq!(state, CaptureLifecycleState::Idle);
    }

    #[test]
    fn custom_path_policy_requires_path() {
        let result = build_recording_artifact_paths(
            "session-1",
            CaptureDestinationPolicy::CustomPath,
            None,
            None,
        );
        assert!(matches!(result, Err(CaptureEngineError::MissingCustomPath)));
    }

    #[test]
    fn app_data_policy_requires_path() {
        let result = build_recording_artifact_paths(
            "session-1",
            CaptureDestinationPolicy::AppData,
            None,
            None,
        );
        assert!(matches!(
            result,
            Err(CaptureEngineError::MissingAppDataPath)
        ));
    }

    #[test]
    fn finalize_recording_wav_writes_file_in_custom_destination() {
        let destination = std::env::temp_dir().join(format!(
            "audio-capture-test-{}",
            chrono::Utc::now().timestamp_millis()
        ));
        let audio = CapturedAudio {
            samples: vec![0.1_f32, -0.1, 0.2, -0.2],
            sample_rate: 16_000,
        };

        let artifacts = finalize_recording_wav(
            &audio,
            "session-1",
            CaptureDestinationPolicy::CustomPath,
            None,
            Some(&destination),
        )
        .expect("expected wav artifact path creation");

        assert!(artifacts.recording_wav_path.exists());
        assert!(artifacts.recording_wav_path.starts_with(&destination));
        assert!(artifacts.transcript_output_path.starts_with(&destination));

        let _ = fs::remove_file(artifacts.recording_wav_path);
        let _ = fs::remove_dir_all(destination);
    }
}
