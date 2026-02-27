# Silero VAD - Rust Implementation

This is a Rust implementation of the [Silero Voice Activity Detection (VAD) model](https://github.com/snakers4/silero-vad). The original model is written in Python and uses PyTorch, while this implementation uses Rust with the `ort` crate for efficient ONNX model inference.

## Features

- Voice Activity Detection (VAD) using the Silero model
- Support for both 8kHz and 16kHz audio
- Streaming VAD with iterator interface and state management
- Batch processing for efficient handling of multiple audio chunks
- GPU acceleration support via ONNX Runtime with CUDA
- Audio file I/O utilities
- Automatic model downloading from Silero repository
- Multiple language support (English, Russian, German, Spanish)
- Comprehensive error handling
- Serialization support for speech timestamps

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
silero-vad-rs = "0.1.0"
```

## Usage

### Basic VAD

```rust
use silero_vad::{SileroVAD, VADIterator};
use silero_vad::utils::{read_audio, save_audio};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the model
    let model = SileroVAD::new("path/to/silero_vad.onnx")?;
    
    // Create a VAD iterator
    let mut vad = VADIterator::new(
        model,
        0.5,  // threshold
        16000, // sampling rate
        100,   // min silence duration (ms)
        30,    // speech pad (ms)
    );

    // Read audio file
    let audio = read_audio("input.wav", 16000)?;
    
    // Get speech timestamps
    let timestamps = vad.get_speech_timestamps(
        &audio.view(),
        250,    // min speech duration (ms)
        f32::INFINITY, // max speech duration (s)
        100,    // min silence duration (ms)
        30,     // speech pad (ms)
    )?;

    // Process timestamps
    for ts in timestamps {
        println!("Speech detected from {:.2}s to {:.2}s", ts.start, ts.end);
    }

    Ok(())
}
```

### Streaming VAD

```rust
use silero_vad::{SileroVAD, VADIterator};
use ndarray::Array1;

fn process_stream() -> Result<(), Box<dyn std::error::Error>> {
    let model = SileroVAD::new("path/to/silero_vad.onnx")?;
    let mut vad = VADIterator::new(model, 0.5, 16000, 100, 30);

    // Process audio chunks
    let chunk_size = 512; // for 16kHz
    let audio_chunk = Array1::zeros(chunk_size);
    
    if let Some(ts) = vad.process_chunk(&audio_chunk.view())? {
        println!("Speech detected from {:.2}s to {:.2}s", ts.start, ts.end);
    }

    Ok(())
}
```

### Batch Processing

```rust
use silero_vad::SileroVAD;
use ndarray::{Array1, ArrayView1};

fn process_batch() -> Result<(), Box<dyn std::error::Error>> {
    let model = SileroVAD::new("path/to/silero_vad.onnx")?;
    
    // Create a batch of audio chunks
    let chunk_size = 512;
    let batch_size = 10;
    let mut chunks = Vec::with_capacity(batch_size);
    
    for _ in 0..batch_size {
        chunks.push(Array1::zeros(chunk_size));
    }
    
    // Process the batch
    let results = model.process_batch(
        &chunks.iter().map(|c| c.view()).collect::<Vec<_>>(),
        16000
    )?;
    
    // Process results
    for (i, prob) in results.iter().enumerate() {
        println!("Chunk {}: speech probability = {:.2}", i, prob[0]);
    }

    Ok(())
}
```

### Audio Utilities

```rust
use silero_vad::utils::{read_audio, save_audio, collect_chunks, drop_chunks};
use silero_vad::{SileroVAD, VADIterator};

fn process_audio() -> Result<(), Box<dyn std::error::Error>> {
    // Read audio file
    let audio = read_audio("input.wav", 16000)?;
    
    // Detect speech segments
    let model = SileroVAD::new("path/to/silero_vad.onnx")?;
    let mut vad = VADIterator::new(model, 0.5, 16000, 100, 30);
    let timestamps = vad.get_speech_timestamps(
        &audio.view(),
        250,
        f32::INFINITY,
        100,
        30,
    )?;
    
    // Extract speech segments
    let speech_only = collect_chunks(&timestamps, &audio, 16000)?;
    save_audio("speech_only.wav", &speech_only, 16000)?;
    
    // Remove speech segments
    let non_speech = drop_chunks(&timestamps, &audio, 16000)?;
    save_audio("non_speech.wav", &non_speech, 16000)?;

    Ok(())
}
```

## Model Files

You need to download the ONNX model file from the [original repository](https://github.com/snakers4/silero-vad). The model supports both 8kHz and 16kHz audio sampling rates.

### Model Variants

- `en_v6_xlarge.onnx` - English model (recommended)
- `ru_v6_xlarge.onnx` - Russian model
- `de_v6_xlarge.onnx` - German model
- `es_v6_xlarge.onnx` - Spanish model

## Performance

The Rust implementation is designed to be efficient and thread-safe. It uses:
- `ort` for optimized ONNX model inference with GPU support
- `ndarray` for efficient array operations
- Zero-copy operations where possible
- Minimal memory allocations

### GPU Acceleration

The library uses ONNX Runtime for GPU acceleration:
1. CUDA acceleration is available when using ONNX Runtime with CUDA support
2. CPU is used if no GPU is available or if CUDA support is not enabled
3. GPU acceleration requires the `cuda` feature of the `ort` crate

## Error Handling

The library provides comprehensive error handling through the `Error` enum:
- `ModelLoad` - Errors during model loading
- `InvalidInput` - Invalid input parameters or data
- `AudioProcessing` - Errors during audio processing
- `Io` - File I/O errors
- `Ort` - ONNX Runtime errors

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Original Silero VAD implementation by [snakers4/silero-vad](https://github.com/snakers4/silero-vad)
- ONNX Runtime binding for Rust by [pykeio/ort](https://github.com/pykeio/ort) 