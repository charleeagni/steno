use silero_vad_rs::{SileroVAD, VADIterator};
use ndarray::Array1;
use std::path::Path;
use std::time::Duration;
use std::thread;
use log::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

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

    println!("Starting streaming VAD simulation...");
    println!("This example simulates processing audio chunks in real-time");
    println!("Press Ctrl+C to exit");

    // Simulate processing audio chunks
    let chunk_size = 512; // for 16kHz
    let mut current_time = 0.0;
    let time_per_chunk = chunk_size as f32 / 16000.0;
    
    // Simulate some audio chunks with speech and silence
    for i in 0..100 {
        // Create a simulated audio chunk
        // In a real application, this would come from a microphone or audio file
        let mut audio_chunk = Array1::zeros(chunk_size);
        
        // Simulate speech in some chunks
        if (i >= 10 && i < 20) || (i >= 40 && i < 50) {
            // Add some signal to simulate speech
            for j in 0..chunk_size {
                audio_chunk[j] = 0.1 * (j as f32 / 10.0).sin();
            }
        }
        
        // Process the chunk
        if let Some(ts) = vad.process_chunk(&audio_chunk.view())? {
            println!("Speech detected from {:.2}s to {:.2}s", ts.start, ts.end);
        }
        
        // Update time
        current_time += time_per_chunk;
        
        // Simulate real-time processing delay
        thread::sleep(Duration::from_millis((time_per_chunk * 1000.0) as u64));
    }

    println!("Streaming VAD simulation completed");
    Ok(())
} 