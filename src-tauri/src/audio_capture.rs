use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct CapturedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub struct AudioCapture {
    stream: Option<Stream>,
    sink: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            sink: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 16_000,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }

    pub fn start(&mut self) -> Result<()> {
        if self.stream.is_some() {
            return Err(anyhow!("Audio capture is already recording"));
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default input device is available"))?;

        let default_config = device
            .default_input_config()
            .context("Failed to read default input stream config")?;
        let sample_format = default_config.sample_format();
        let config: cpal::StreamConfig = default_config.clone().into();
        let channels = usize::from(config.channels);

        let sink = Arc::new(Mutex::new(Vec::new()));
        self.sink = sink.clone();
        self.sample_rate = config.sample_rate.0;

        let err_fn = |err| {
            log::error!("event=audio_stream_error error={}", err);
        };

        let stream = match sample_format {
            SampleFormat::F32 => {
                let sink = sink.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        push_f32_mono(data, channels, &sink);
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                let sink = sink.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        push_i16_mono(data, channels, &sink);
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                let sink = sink.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        push_u16_mono(data, channels, &sink);
                    },
                    err_fn,
                    None,
                )?
            }
            other => {
                return Err(anyhow!(
                    "Unsupported input sample format for v1 recorder: {:?}",
                    other
                ));
            }
        };

        stream.play().context("Failed to start microphone stream")?;
        self.stream = Some(stream);

        Ok(())
    }

    pub fn stop(&mut self) -> Result<CapturedAudio> {
        let Some(stream) = self.stream.take() else {
            return Err(anyhow!("Audio capture is not currently recording"));
        };

        // Drop the stream to stop the callback pipeline.
        drop(stream);

        let mut guard = self
            .sink
            .lock()
            .map_err(|_| anyhow!("Audio sample sink lock poisoned"))?;
        let samples = std::mem::take(&mut *guard);

        Ok(CapturedAudio {
            samples,
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

pub fn write_wav_file(path: &Path, audio: &CapturedAudio) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: WavSampleFormat::Int,
    };

    let mut writer = WavWriter::create(path, spec)
        .with_context(|| format!("Failed to create WAV file at {}", path.display()))?;

    for sample in &audio.samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_sample = (clamped * i16::MAX as f32) as i16;
        writer
            .write_sample(i16_sample)
            .context("Failed to write WAV sample")?;
    }

    writer.finalize().context("Failed to finalize WAV file")?;
    Ok(())
}
