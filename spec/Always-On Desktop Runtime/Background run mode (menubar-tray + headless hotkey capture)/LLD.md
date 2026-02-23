# LLD: CODIN-168 Background Run Mode

## Scope
Implement background run behavior for the existing Tauri app by introducing tray controls and non-destructive window close semantics, while preserving current runtime and hotkey architecture.

## Constraints
- Keep functionality local to desktop shell/runtime boundaries.
- Do not change transcription pipeline logic.
- Do not introduce new service layers or abstractions.

## File-Level Plan

### 1) `src-tauri/src/lib.rs`
Update Tauri builder setup for background lifecycle behavior.

Planned changes:
- Add tray creation and menu wiring in app setup.
- Add tray event handlers:
  - Show Steno: show + focus `main` window.
  - Hide Steno: hide `main` window.
  - Quit Steno: call app exit.
- Replace current close behavior by intercepting main-window close requests:
  - prevent default close
  - hide window instead
- Keep runtime state emission during setup unchanged.

Expected effect:
- App no longer exits when user closes window.
- App remains controllable from tray.

### 2) `src-tauri/tauri.conf.json` (only if required by runtime)
If tray/icon behavior needs explicit config flags, add minimal required configuration.

Expected effect:
- Stable tray icon and background operation in packaged app.

### 3) `src-tauri/src/runtime.rs`
No behavior change expected.

Reason:
- Runtime hotkey lifecycle is already process-scoped.
- Hidden window should not impact runtime controller.

Potential tiny adjustment (only if needed after testing):
- Add log lines around hidden-mode state transitions for debugging.

## Lifecycle Flow

### Launch
- Process starts.
- Runtime controller managed as singleton.
- Frontend initializes runtime and attempts shortcut registration.
- Tray becomes available.

### Close Window
- User clicks window close button.
- Close event intercepted.
- Main window hidden; process remains alive.
- Runtime and shortcut manager continue running.

### Restore
- User clicks tray item Show Steno.
- Main window shown and focused.
- Frontend receives latest runtime state via existing initialization/state events.

### Quit
- User clicks tray item Quit Steno.
- App exits normally.
- Runtime drops and shortcut manager unregisters hotkey through existing Drop logic.

## State/Contract Impact
- No new runtime enum variants.
- No new command interface required for frontend.
- Existing events remain authoritative:
  - `steno://state-changed`
  - `steno://error`
  - `steno://transcription-complete`

## Acceptance Criteria
- Closing window does not terminate app.
- Tray menu can show, hide, and quit app.
- Fn shortcut works while app window is hidden.
- Recording/transcription completes in hidden mode.
- Restored window reflects current runtime state.
- Quit fully terminates process and shortcut listener.

## Test Plan

### Manual checks
- Start app, initialize runtime, verify shortcut ready.
- Hide window via close button, press Fn workflow, validate transcript success.
- Restore from tray and verify state consistency.
- Use tray quit and confirm app fully exits.

### Regression checks
- Manual start/stop recording commands from visible UI still work.
- Permission denied and shortcut init failure paths still surface errors.

## Rollback Strategy
- Revert tray and close-intercept changes in `src-tauri/src/lib.rs`.
- Restore existing always-show-on-setup behavior.

## Open Questions
- Should first launch default to visible window or background hidden mode?
- Should dock icon visibility behavior change, or remain default for now?
