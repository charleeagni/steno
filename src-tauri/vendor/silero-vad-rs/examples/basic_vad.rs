use silero_vad_rs::{SileroVAD, VADIterator};
use silero_vad_rs::utils::{read_audio, save_audio};
use std::path::Path;
use log::info;
use std::time::Instant;
use rayon::prelude::*;
use ndarray::{ArrayView1, s, Array2};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();
    
    // Start timing
    let start_time = Instant::now();
    
    // Load the model (will be downloaded automatically if not present)
    info!("Loading VAD model...");
    let model_path = Path::new("models/silero_vad.onnx");
    let model = SileroVAD::new(model_path)?;
    
    // Create a VAD iterator
    let mut vad = VADIterator::new(
        model,
        0.5,  // threshold
        16000, // sampling rate
        100,   // min silence duration (ms)
        30,    // speech pad (ms)
    );

    // Check if input file exists
    let input_path = Path::new("examples/input.wav");
    if !input_path.exists() {
        println!("Input file not found. Please provide a WAV file at examples/input.wav");
        return Ok(());
    }

    // Read audio file
    println!("Reading audio file...");
    let audio = read_audio(input_path, 16000)?;
    println!("Audio loaded: {} samples", audio.len());
    
    // Get speech timestamps using optimized batch processing
    println!("Detecting speech...");
    let timestamps = optimized_get_speech_timestamps(
        &mut vad,
        &audio.view(),
        250,    // min speech duration (ms)
        f32::INFINITY, // max speech duration (s)
        100,    // min silence duration (ms)
        30,     // speech pad (ms)
    )?;

    // Process timestamps
    println!("Found {} speech segments:", timestamps.len());
    for (i, ts) in timestamps.iter().enumerate() {
        println!("  {}. {:.2}s - {:.2}s (duration: {:.2}s)", 
                 i+1, ts.start, ts.end, ts.end - ts.start);
    }

    // Save speech segments to separate files in parallel
    if !timestamps.is_empty() {
        println!("Saving speech segments...");
        timestamps.par_iter().enumerate().for_each(|(i, ts)| {
            let output_path = format!("examples/speech_{}.wav", i+1);
            let speech_audio = silero_vad_rs::utils::collect_chunks(&[ts.clone()], &audio, 16000).unwrap();
            save_audio(&output_path, &speech_audio, 16000).unwrap();
            println!("  Saved segment {} to {}", i+1, output_path);
        });
    }

    // Print total processing time
    let elapsed = start_time.elapsed();
    println!("Total processing time: {:.2}s", elapsed.as_secs_f64());

    Ok(())
}

/// Optimized version of get_speech_timestamps that uses batch processing
fn optimized_get_speech_timestamps(
    vad: &mut VADIterator,
    audio: &ArrayView1<f32>,
    min_speech_duration_ms: u32,
    max_speech_duration_s: f32,
    _min_silence_duration_ms: u32,
    _speech_pad_ms: u32,
) -> std::result::Result<Vec<silero_vad_rs::SpeechTimestamps>, silero_vad_rs::Error> {
    let mut timestamps = Vec::new();
    let chunk_size = 512; // Fixed chunk size for 16kHz (model requirement)
    let batch_size = 128; // Increased batch size for better throughput
    
    // Process audio in batches
    let mut i = 0;
    while i < audio.len() {
        let mut batch_chunks = Vec::with_capacity(batch_size);
        let mut batch_positions = Vec::with_capacity(batch_size);
        
        // Prepare a batch of chunks
        for _ in 0..batch_size {
            if i >= audio.len() {
                break;
            }
            
            let end = (i + chunk_size).min(audio.len());
            if end - i < chunk_size {
                break;
            }
            
            let window = audio.slice(s![i..end]);
            batch_chunks.push(window);
            batch_positions.push(i);
            i = end;
        }
        
        if batch_chunks.is_empty() {
            break;
        }
        
        // Convert batch to a single tensor for processing
        let mut batch_tensor = Array2::zeros((batch_chunks.len(), chunk_size));
        for (j, chunk) in batch_chunks.iter().enumerate() {
            batch_tensor.slice_mut(s![j, ..]).assign(chunk);
        }
        
        // Process the entire batch at once
        if let Some(batch_results) = vad.process_batch(&batch_tensor)? {
            // Filter and adjust timestamps
            for ts in batch_results {
                if ts.end - ts.start >= min_speech_duration_ms as f32 / 1000.0
                    && ts.end - ts.start <= max_speech_duration_s
                {
                    timestamps.push(ts);
                }
            }
        }
    }
    
    Ok(timestamps)
} 