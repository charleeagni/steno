.PHONY: all build test clean doc format lint check bench run-example run-streaming setup generate-test-audio

# Default target
all: build

# Build the project
build:
	cargo build

# Build in release mode
release:
	cargo build --release

# Run tests
test:
	cargo test

# Clean build artifacts
clean:
	cargo clean

# Generate documentation
doc:
	cargo doc --no-deps --open

# Format code
format:
	cargo fmt --all

# Run linter
lint:
	cargo clippy -- -D warnings

# Check for errors without building
check:
	cargo check

# Run benchmarks
bench:
	cargo bench

# Run basic example
run-example:
	cargo run --example basic_vad

# Run basic example with debug logging
run-example-debug:
	RUST_LOG=debug cargo run --example basic_vad --release

# Run streaming example
run-streaming:
	cargo run --example streaming_vad

# Run streaming example with debug logging
run-streaming-debug:
	RUST_LOG=debug cargo run --example streaming_vad

# Run streaming example in release mode
run-streaming-release:
	cargo run --example streaming_vad --release

# Generate test audio
generate-test-audio:
	cargo run --example generate_test_audio

# Setup the project (install dependencies and build)
setup: install-dev-deps build generate-test-audio

# Install development dependencies
install-dev-deps:
	rustup component add rustfmt
	rustup component add clippy

# Create a new example
new-example:
	@if [ -z "$(name)" ]; then \
		echo "Usage: make new-example name=example_name"; \
		exit 1; \
	fi
	@mkdir -p examples
	@touch examples/$(name).rs
	@echo "Created new example at examples/$(name).rs"

# Run all checks (format, lint, test)
check-all: format lint test

# Default target
.DEFAULT_GOAL := all 