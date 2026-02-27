use std::fs;
use tempfile::TempDir;
use ort::session::{Session, builder::GraphOptimizationLevel};

const MODEL_URL: &str = "https://models.silero.ai/models/en/en_v6_xlarge.onnx";

#[test]
fn test_model_auto_download() {
    // Create a temporary directory for the test
    let temp_dir = TempDir::new().unwrap();
    let model_path = temp_dir.path().join("silero_vad.onnx");
    
    // Ensure the model doesn't exist initially
    assert!(!model_path.exists(), "Model file should not exist before the test");
    
    // Download the model using ort's fetch-models feature
    println!("Downloading model from {}", MODEL_URL);
    let _session = Session::builder()
        .unwrap()
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .unwrap()
        .commit_from_url(MODEL_URL)
        .unwrap();
    
    println!("Model downloaded successfully");
    
    // Verify that the model was downloaded to the cache directory
    let cache_dir = dirs::cache_dir().unwrap().join("ort.pyke.io/models");
    let model_files: Vec<_> = fs::read_dir(&cache_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .collect();
    
    assert!(!model_files.is_empty(), "Model should be downloaded to the cache directory");
    
    // Clean up is handled automatically by TempDir when it goes out of scope
} 