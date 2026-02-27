//! Silero VAD model implementation
//! 
//! This module provides the core Silero VAD model implementation using the ONNX runtime.
//! It supports both single chunk and batch processing of audio data.

use crate::{Error, Result};
use ndarray::{Array1, Array2, Array3, ArrayView1};
use std::path::Path;
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use log::{info, debug};
use std::fs;

/// Main Silero VAD model wrapper
/// 
/// This struct provides the core functionality for voice activity detection using the Silero model.
/// It supports both GPU acceleration via TensorRT/CUDA and CPU inference.
/// 
/// # Example
/// 
/// ```rust
/// use silero_vad::SileroVAD;
/// use ndarray::Array1;
/// 
/// let model = SileroVAD::new("path/to/model.onnx")?;
/// let audio_chunk = Array1::zeros(512); // 512 samples for 16kHz
/// let speech_prob = model.process_chunk(&audio_chunk.view(), 16000)?;
/// ```
pub struct SileroVAD {
    session: Session,
    context: Array2<f32>,
    state: Array3<f32>,
    last_sr: u32,
    last_batch_size: usize,
}

impl SileroVAD {
    /// Create a new Silero VAD model from an ONNX file
    /// 
    /// # Arguments
    /// 
    /// * `model_path` - Path to the ONNX model file. If the file doesn't exist,
    ///                  it will be downloaded from the Silero model repository.
    /// 
    /// # Returns
    /// 
    /// A new `SileroVAD` instance ready for inference
    /// 
    /// # Errors
    /// 
    /// Returns an error if:
    /// * The model file cannot be loaded or downloaded
    /// * The model is invalid or incompatible
    /// * GPU initialization fails (falls back to CPU)
    pub fn new(model_path: &Path) -> Result<Self> {
        // Create models directory if it doesn't exist
        if let Some(parent) = model_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Load model with CPU execution provider.
        let session = if model_path.exists() {
            info!("Loading model from local file: {:?}", model_path);
            Session::builder()?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_intra_threads(1)?
                .commit_from_file(model_path)?
        } else {
            return Err(Error::ModelLoad(format!(
                "Model not found at {}. Automatic download is unavailable.",
                model_path.display()
            )));
        };

        info!("Model loaded successfully");

        Ok(Self {
            session,
            context: Array2::zeros((1, 64)),
            state: Array3::zeros((2, 1, 128)),
            last_sr: 0,
            last_batch_size: 0,
        })
    }

    /// Reset the model's internal state
    /// 
    /// This should be called when processing a new audio stream or when
    /// the batch size changes.
    /// 
    /// # Arguments
    /// 
    /// * `batch_size` - The new batch size for processing
    pub fn reset_states(&mut self, batch_size: usize) {
        self.context = Array2::zeros((batch_size, 64));
        self.state = Array3::zeros((2, batch_size, 128));
    }

    /// Validate input audio chunk
    /// 
    /// # Arguments
    /// 
    /// * `x` - Audio chunk to validate
    /// * `sr` - Sampling rate of the audio
    /// 
    /// # Returns
    /// 
    /// `Ok(())` if the input is valid, `Err` otherwise
    fn validate_input(&self, x: &ArrayView1<f32>, sr: u32) -> Result<()> {
        if sr != 16000 {
            return Err(Error::InvalidInput("Sampling rate must be 16kHz".into()));
        }
        if x.len() != 512 {
            return Err(Error::InvalidInput("Input chunk must be 512 samples".into()));
        }
        Ok(())
    }

    /// Process a single audio chunk
    /// 
    /// # Arguments
    /// 
    /// * `x` - Audio chunk to process (must be 512 samples for 16kHz)
    /// * `sr` - Sampling rate of the audio (must be 16kHz)
    /// 
    /// # Returns
    /// 
    /// Speech probability for the chunk
    /// 
    /// # Errors
    /// 
    /// Returns an error if:
    /// * The input chunk size is invalid
    /// * The sampling rate is not supported
    /// * Model inference fails
    pub fn process_chunk(&mut self, x: &ArrayView1<f32>, sr: u32) -> Result<Array1<f32>> {
        self.validate_input(x, sr)?;

        let batch_size = 1;
        if self.last_batch_size != batch_size {
            self.reset_states(batch_size);
        }

        if self.last_sr != 0 && self.last_sr != sr {
            self.reset_states(batch_size);
        }

        // Prepare input tensor
        let input = Array2::from_shape_fn((batch_size, x.len() + 64), |(i, j)| {
            if j < 64 {
                self.context[[i, j]]
            } else {
                x[j - 64]
            }
        });

        // Create input tensor
        let input_shape = input.shape().to_vec();
        let input_data = input.into_raw_vec();

        debug!("Processing input tensor of shape {:?}", input_shape);

        // Create input tensor with just the 'input' name
        let inputs = vec![
            ("input", Tensor::from_array((input_shape, input_data.clone()))?.into_dyn()),
            ("sr", Tensor::from_array(([1usize], vec![sr as i64]))?.into_dyn()),
            (
                "state",
                Tensor::from_array(([2usize, batch_size, 128usize], self.state.clone().into_raw_vec()))?
                    .into_dyn(),
            ),
        ];

        let outputs = self.session.run(inputs)?;
        
        // Update context from the last 64 elements of input_data
        let context_data = input_data[input_data.len()-64..].to_vec();
        self.context = Array2::from_shape_vec((batch_size, 64), context_data)
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        let state_tensor = outputs[1].try_extract_tensor::<f32>()?;
        self.state = Array3::from_shape_vec((2, batch_size, 128), state_tensor.1.to_vec())
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        
        self.last_sr = sr;
        self.last_batch_size = batch_size;

        // Return speech probability
        let output_tensor = outputs[0].try_extract_tensor::<f32>()?;
        Ok(Array1::from_vec(output_tensor.1.to_vec()))
    }

    /// Process a batch of audio chunks
    /// 
    /// # Arguments
    /// 
    /// * `x` - Batch of audio chunks to process (each chunk must be 512 samples for 16kHz)
    /// * `sr` - Sampling rate of the audio (must be 16kHz)
    /// 
    /// # Returns
    /// 
    /// Speech probabilities for each chunk in the batch
    /// 
    /// # Errors
    /// 
    /// Returns an error if:
    /// * The input chunk size is invalid
    /// * The sampling rate is not supported
    /// * Model inference fails
    pub fn process_batch(&mut self, x: &Array2<f32>, sr: u32) -> Result<Array1<f32>> {
        if sr != 16000 {
            return Err(Error::InvalidInput("Sampling rate must be 16kHz".into()));
        }
        if x.ncols() != 512 {
            return Err(Error::InvalidInput("Input chunks must be 512 samples".into()));
        }

        let batch_size = x.nrows();
        if self.last_batch_size != batch_size {
            self.reset_states(batch_size);
        }

        if self.last_sr != 0 && self.last_sr != sr {
            self.reset_states(batch_size);
        }

        // Prepare input tensor
        let input = Array2::from_shape_fn((batch_size, x.ncols() + 64), |(i, j)| {
            if j < 64 {
                self.context[[i, j]]
            } else {
                x[[i, j - 64]]
            }
        });

        // Create input tensor
        let input_shape = input.shape().to_vec();
        let input_data = input.into_raw_vec();

        debug!("Processing batch input tensor of shape {:?}", input_shape);

        // Create input tensor with just the 'input' name
        let inputs = vec![
            ("input", Tensor::from_array((input_shape, input_data.clone()))?.into_dyn()),
            ("sr", Tensor::from_array(([1usize], vec![sr as i64]))?.into_dyn()),
            (
                "state",
                Tensor::from_array(([2usize, batch_size, 128usize], self.state.clone().into_raw_vec()))?
                    .into_dyn(),
            ),
        ];

        let outputs = self.session.run(inputs)?;
        
        // Update context from the last 64 elements of input_data
        let context_data = input_data[input_data.len()-64*batch_size..].to_vec();
        self.context = Array2::from_shape_vec((batch_size, 64), context_data)
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        let state_tensor = outputs[1].try_extract_tensor::<f32>()?;
        self.state = Array3::from_shape_vec((2, batch_size, 128), state_tensor.1.to_vec())
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        
        self.last_sr = sr;
        self.last_batch_size = batch_size;

        // Return speech probabilities
        let output_tensor = outputs[0].try_extract_tensor::<f32>()?;
        Ok(Array1::from_vec(output_tensor.1.to_vec()))
    }
} 
