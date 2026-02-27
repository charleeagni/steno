#!/bin/bash

# Exit on error
set -e

echo "Setting up Silero VAD Rust project..."

# Check if Rust is installed
if ! command -v rustc &> /dev/null; then
    echo "Rust is not installed. Please install Rust first: https://rustup.rs/"
    exit 1
fi

# Install development dependencies
echo "Installing development dependencies..."
rustup component add rustfmt
rustup component add clippy

# Build the project
echo "Building the project..."
cargo build

# Create example directory if it doesn't exist
mkdir -p examples

# Generate test audio
echo "Generating test audio..."
cargo run --example generate_test_audio

# Check if examples/input.wav exists
if [ ! -f "examples/input.wav" ]; then
    echo "No input.wav file found in examples directory."
    echo "Please place a WAV file at examples/input.wav to run the examples."
fi

echo "Setup complete!"
echo "To run the basic example: make run-example"
echo "To run the streaming example: make run-streaming"
echo "To run all tests: make test"
echo "To format code: make format"
echo "To run linter: make lint" 