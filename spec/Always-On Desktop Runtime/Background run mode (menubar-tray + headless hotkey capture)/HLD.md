# HLD: CODIN-168 Background Run Mode

## Overview
Enable Steno to run as an always-available desktop utility in macOS without requiring the main window to stay focused or visible.

The design keeps runtime ownership in the existing Rust `RuntimeController` and introduces app-shell behavior for:
- launch behavior
- hide/restore behavior
- menubar/tray control surface
- persistent hotkey readiness while headless

## Goals
- Keep global hotkey capture active when the window is hidden.
- Allow users to restore the app quickly from tray/menubar.
- Support launch-at-login style operation where app can remain available in background.
- Preserve current recording/transcription state model and event contracts.

## Non-Goals
- No hotkey remapping UX.
- No new transcription engine behavior.
- No changes to pipeline logic outside app lifecycle and visibility handling.

## Current Baseline
- `RuntimeController` owns recording state and Fn hotkey handling.
- Hotkey initialization occurs in `initialize_runtime` through `initialize_shortcut_with_timeout`.
- Setup currently forces main window show and focus.
- There is no tray/menubar interaction path to control visibility.

## Target Behavior

### 1) App Launch Mode
- App process starts and initializes runtime as today.
- Window display behavior is policy driven:
  - normal launch: show window
  - background launch: keep window hidden
- Runtime and hotkey manager stay process-scoped, independent of window visibility.

### 2) Hide/Restore Semantics
- Closing the main window hides it instead of terminating app process.
- Hide action must not tear down `RuntimeController` or shortcut manager.
- Restore action from tray/menubar shows and focuses main window.

### 3) Menubar/Tray Surface
- Add tray icon with minimal menu:
  - Show Steno
  - Hide Steno
  - Quit Steno
- Tray menu actions operate only on window lifecycle and app exit.

### 4) Headless Hotkey Readiness
- While hidden, Fn shortcut remains registered.
- Recording/transcription can run normally in hidden mode.
- State events and error events continue to emit for frontend sync once restored.

## Architecture Decisions
- Keep runtime singleton placement unchanged (`.manage(RuntimeController::new())`).
- Add tray and window lifecycle policy in Tauri app setup layer.
- Keep recording logic in `runtime.rs`; no cross-module logic moves.
- Model “headless availability” via process/window lifecycle state, not by duplicating runtime states.

## Failure Handling
- If shortcut initialization fails, app remains launchable and visible with actionable error.
- If tray creation fails, fallback is normal window mode with logging.
- Quit action always performs full process shutdown.

## Validation Strategy
- Manual verification cases:
  - Launch app, hide window, trigger shortcut recording, restore and confirm state.
  - Close window and verify process remains active via tray.
  - Quit from tray and confirm full shutdown.
  - Shortcut init error path still allows opening window and remediation.

## Risks
- macOS behavior differences between close, hide, and terminate events.
- Tray availability nuances depending on Tauri configuration and packaging.
- Race between window lifecycle and frontend state resubscription on restore.

## Rollout
- Implement behind default behavior that still supports visible launch.
- Keep user-facing change minimal: tray controls + non-destructive window close.
- Defer launch-at-login toggle UX to later task if needed.
