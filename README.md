# Steno
Lean Handy-style local transcriber for macOS.

## What v1 does
- Uses `rust-transcriber` (Whisper path) via path dependency.
- Global `Fn` hotkey support only.
- Two record modes:
- `push_to_talk`: hold `Fn` to record, release to transcribe.
- `toggle`: press `Fn` once to start, again to stop/transcribe.
- Copies transcript to clipboard and shows it in the app UI.
- Includes manual start/stop button wired to the same backend flow.

## What v1 does not do
- No Parakeet runtime path.
- No auto-paste into active app.
- No persistent recording/transcript history.
- No non-macOS support.
- No fallback shortcut key if `Fn` capture fails.

## Run
1. Install dependencies:
```bash
npm install
```
2. Run the app:
```bash
npm run tauri dev
```

## Build checks
```bash
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

## Permission model
- On startup, UI checks microphone permission (`tauri-plugin-macos-permissions-api`).
- If missing, user is prompted to grant microphone access.
- Runtime then initializes global `Fn` capture.
- If `Fn` shortcut initialization fails, app shows guidance for Input Monitoring / Accessibility and blocks further usage.

## Backend flow
- `Fn` or manual button triggers runtime state machine:
- `Idle -> Recording -> Transcribing -> Idle/Error`
- Audio is recorded on demand and written to a temp WAV file.
- Transcription uses:
- `transcriber_core::transcribe_file(input_path, output_path, model_id)`
- Output is post-processed through a no-op interface (future LLM hook), then copied to clipboard.

## Key files
- `/Users/karthik/Desktop/merge_conflicts/Personal Automation/steno/src-tauri/src/runtime.rs`
- `/Users/karthik/Desktop/merge_conflicts/Personal Automation/steno/src-tauri/src/shortcut.rs`
- `/Users/karthik/Desktop/merge_conflicts/Personal Automation/steno/src-tauri/src/audio_capture.rs`
- `/Users/karthik/Desktop/merge_conflicts/Personal Automation/steno/src/App.tsx`
