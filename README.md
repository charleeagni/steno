# Steno

Local, macOS-first dictation app built with Tauri, React, and Rust.

Steno records audio from a global shortcut, transcribes locally, and inserts text into the active app.

## Current Scope

- macOS only.
- Local transcription runtimes:
  - `whisper` (downloaded model artifacts managed in-app)
  - `parakeet` (Rust runtime with downloadable ONNX model variants)
  - `moonshine` (Rust runtime with downloaded ONNX tiny/base variants)
- Two recording modes:
  - `push_to_talk` (`Fn` by default)
  - `toggle` (`Shift+Fn` by default)
- Background tray app behavior (close hides window, app keeps running).
- No persistent transcript history.

## Tech Stack

- Frontend: React + TypeScript + Vite (`src/`)
- Desktop shell: Tauri v2 (`src-tauri/`)
- Transcription core: `transcriber-core` path dependency (Rust)

## Prerequisites

- macOS
- Node.js + npm
- Rust toolchain (`rustup`, `cargo`)
- Xcode Command Line Tools
- Local `rust-transcriber` checkout available at:
  - `../../rust-transcriber/crates/transcriber-core`
  - This path is required by `src-tauri/Cargo.toml`
- Bundled Silero VAD model file:
  - `assets/models/silero_vad.onnx`
  - Recording start fails if this file is missing or invalid.

## Quick Start

```bash
npm install
npm run tauri dev
```

On first run, grant permissions when prompted:

- Microphone
- Accessibility
- Input Monitoring

Notes:

- First Rust build with Silero may take longer because ONNX Runtime binaries are fetched by `ort`.
- If macOS linker/rpath issues appear, enable `rpath = true` for dev/release profiles in `.cargo/config.toml`.

## Build

Frontend-only build:

```bash
npm run build
```

Desktop app build:

```bash
npm run tauri build
```

Expected output locations:

- App bundle: `src-tauri/target/release/bundle/macos/`
- Installer artifacts (for distribution): `src-tauri/target/release/bundle/dmg/`

## macOS Release Plan (Downloadable Build)

For a user-downloadable build, use this baseline flow:

1. Bump versions consistently:
   - `package.json` (`version`)
   - `src-tauri/tauri.conf.json` (`version`)
   - `src-tauri/Cargo.toml` (`version`)
2. Build release artifacts:
   - `npm run tauri build`
3. Sign and notarize artifacts (required to avoid Gatekeeper friction).
4. Publish `.dmg` in a release channel (for example GitHub Releases).
5. Include first-run permission instructions in release notes.

## Runtime and Permissions

- If Input Monitoring is denied, global shortcut capture is disabled.
- Manual recording controls remain available without Input Monitoring.
- Accessibility is required for reliable shortcut and output behavior.
- Clipboard policy supports:
  - restore previous clipboard
  - keep transcript in clipboard

## Repository Map

- `src/App.tsx`: main UI and startup orchestration.
- `src/OverlayApp.tsx`: recording overlay window.
- `src/tauri.ts`: typed command/event bridge.
- `src-tauri/src/lib.rs`: Tauri shell, tray behavior, command registration.
- `src-tauri/src/runtime.rs`: runtime state machine and transcription/output flow.
- `src-tauri/src/model_download.rs`: model download queue/state for Whisper, Parakeet, and Moonshine.
- `spec/how-it-works/understanding this codebase/README.md`: deep technical walkthrough.
