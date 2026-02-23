# Low Level Design: CODIN-171 Recording Visual Indicator Overlay

## Objective
Implement a minimal floating overlay window that provides immediate visual feedback while recording is active. The overlay must be driven by existing runtime phase events and must not change recording/transcription logic.

## Constraints
- Keep changes local to Steno app runtime and frontend entry rendering.
- Do not add waveform support.
- Do not alter runtime phase semantics.
- Do not introduce new persistence, settings schema, or external dependencies.

## Current Baseline
- One Tauri window (`main`) is configured in `src-tauri/tauri.conf.json`.
- Runtime emits `steno://state-changed` from `src-tauri/src/runtime.rs`.
- Frontend main window subscribes to state in `src/App.tsx`.
- Frontend has a single entry route in `src/main.tsx`.

## Design Decision
Use a dedicated overlay window label with a lightweight frontend overlay view. Keep state ownership in Rust runtime and propagate state to both windows through the existing event channel.

## Detailed Design

### 1) Tauri Window Definition
File: `src-tauri/tauri.conf.json`
- Add a second window definition with label `overlay`.
- Window properties:
  - Always on top.
  - Transparent and decorationless.
  - Non-resizable.
  - Small fixed size suitable for status chip.
  - Hidden by default at app start.
  - Visible on all workspaces only if already supported cleanly by current platform behavior.
- Keep `main` window behavior unchanged.

Rationale:
A dedicated overlay window avoids coupling floating behavior with the main UI layout and keeps implementation readable.

### 2) Overlay Window Lifecycle Controller
File: `src-tauri/src/lib.rs`
- In `setup`, resolve both windows (`main` and `overlay`).
- Keep existing `main` show/focus behavior.
- Ensure overlay starts hidden.
- Add minimal helper-level logic in setup or a small local function to:
  - Emit initial runtime state once app starts.
  - Apply initial overlay visibility from current phase.

Rationale:
Centralizing window boot behavior in `setup` keeps lifecycle decisions in one place.

### 3) Runtime-Driven Overlay Visibility
File: `src-tauri/src/runtime.rs`
- Reuse existing phase transitions in:
  - `start_recording`
  - `stop_recording_and_transcribe`
  - error publication paths
- Extend the existing state emission flow to also trigger overlay visibility updates.
- Add a focused internal function to map phase to overlay visibility:
  - `recording` => show overlay
  - `idle` => hide overlay
  - `error` => hide overlay
  - `transcribing` => hide overlay in this iteration
- Keep event emission (`steno://state-changed`) unchanged.

Rationale:
Overlay behavior must follow the same source of truth used by frontend state rendering.

### 4) Frontend Overlay View Routing
File: `src/main.tsx`
- Add a lightweight runtime branch that detects window context.
- Render standard `App` for `main` label.
- Render new `OverlayApp` component for `overlay` label.
- Keep single bundle and avoid introducing router dependencies.

Rationale:
Window-label-based rendering keeps architecture simple with minimal boot complexity.

### 5) Overlay Component
New file: `src/OverlayApp.tsx`
- Responsibilities:
  - Subscribe to `steno://state-changed` using existing `events.onStateChanged` helper.
  - Maintain local `runtimeState` with existing `RuntimeState` type.
  - Render compact indicator content only when phase is `recording`.
  - For non-recording phases, render an empty transparent container.
- Component should not provide controls.
- Component should not mutate runtime state.

Rationale:
Overlay remains presentation-only and cannot affect recording lifecycle.

### 6) Overlay Styles
File: `src/styles.css`
- Add isolated overlay styles with dedicated class names.
- Visual behavior:
  - High-contrast recording pill.
  - Small pulse indicator is acceptable but optional.
  - Transparent background outside indicator.
- Ensure existing main app styles are not regressed.

Rationale:
Single stylesheet keeps footprint small while preserving readability.

## File Change Plan
- Update `src-tauri/tauri.conf.json`.
- Update `src-tauri/src/lib.rs`.
- Update `src-tauri/src/runtime.rs`.
- Update `src/main.tsx`.
- Add `src/OverlayApp.tsx`.
- Update `src/styles.css`.

## State Mapping Contract
- Input state: existing `RuntimeState.phase`.
- Output behavior:
  - `idle`: overlay hidden.
  - `recording`: overlay visible with "Recording" label.
  - `transcribing`: overlay hidden.
  - `error`: overlay hidden.

No new type definitions are required.

## Error Handling
- If overlay window lookup fails, log a structured error and continue with main window.
- Overlay visibility update failures must not fail recording operations.
- Event subscription failures in overlay should fail closed (no overlay), not crash runtime.

## Test Plan

### Manual Functional Tests
- Launch app:
  - Main window opens.
  - Overlay is hidden.
- Start recording via main button:
  - Overlay appears immediately.
- Stop recording:
  - Overlay hides immediately.
- Start recording via Fn shortcut:
  - Overlay appears without interacting with main UI.
- Trigger recoverable runtime error path:
  - Overlay does not remain stuck visible.

### Regression Checks
- Main status label still updates correctly.
- Manual start/stop behavior unchanged.
- Transcription completion and clipboard behavior unchanged.

## Rollout and Rollback
- Rollout: ship as default behavior with no feature flag.
- Rollback: remove overlay window config and overlay-specific visibility calls; main app remains unaffected.

## Deferred Items
- `transcribing` overlay state treatment beyond hide.
- Waveform/audio level rendering.
- User-configurable position, opacity, and size.
- Multi-monitor placement strategy.
