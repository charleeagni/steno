use hound::{WavSpec, WavWriter};
use std::f32::consts::PI;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a WAV file with speech and silence segments
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    
    let mut writer = WavWriter::create("examples/input.wav", spec)?;
    
    // Generate 5 seconds of audio
    let duration_seconds = 5.0;
    let samples_per_second = 16000;
    let total_samples = (duration_seconds * samples_per_second as f32) as usize;
    
    // Create speech segments at 1-2s and 3-4s
    for i in 0..total_samples {
        let t = i as f32 / samples_per_second as f32;
        let sample = if (t >= 1.0 && t < 2.0) || (t >= 3.0 && t < 4.0) {
            // Speech segment - generate a simple sine wave
            0.5 * (2.0 * PI * 440.0 * t).sin()
        } else {
            // Silence segment
            0.0
        };
        
        // Convert to i16 and write
        let sample_i16 = (sample * 32767.0) as i16;
        writer.write_sample(sample_i16)?;
    }
    
    writer.finalize()?;
    println!("Generated test audio file: examples/input.wav");
    println!("Contains speech segments at 1-2s and 3-4s");
    
    Ok(())
} 