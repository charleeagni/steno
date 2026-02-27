# Low Level Design: CODIN-172 Text Insertion and Selection Replacement in Active App

## Objective
Implement auto-paste output after transcription with configurable clipboard handling and observable output metadata, while keeping correction-time reselection as design-only.

## Constraints
- Keep changes local to runtime, command surface, and existing main UI.
- Preserve existing transcription phase flow.
- Do not add a separate replace action.
- Do not add persistent settings storage.

## Detailed Design

### 1) Runtime Types and State
File: `src-tauri/src/runtime.rs`

Add new enums:
- ClipboardPolicy:
  - restore_previous
  - keep_transcript
- OutputStatus:
  - auto_pasted
  - paste_failed
  - copied_only

Extend RuntimeState:
- Add `clipboard_policy`.
- Default to `restore_previous`.

Extend TranscriptionResult:
- Add `output_status`.
- Add `clipboard_restored` as nullable bool.

### 2) Clipboard Policy Update API
File: `src-tauri/src/runtime.rs`

Add method:
- `set_clipboard_policy(app, policy)`
- Updates runtime state and emits state event.

### 3) Output Pipeline in stop_recording_and_transcribe
File: `src-tauri/src/runtime.rs`

After final transcript text is produced:
1. Read current `clipboard_policy` from runtime state.
2. If policy is restore_previous:
   - Best-effort read clipboard text snapshot.
   - On failure, emit recoverable warning only.
3. Write transcript text to clipboard.
   - On failure, mark output_status paste_failed and emit recoverable warning.
4. If clipboard write succeeded:
   - Trigger paste via macOS System Events keystroke command.
   - On success, mark output_status auto_pasted.
   - On failure, mark output_status paste_failed and emit recoverable warning.
5. If policy is restore_previous and snapshot exists:
   - Attempt restore.
   - Set clipboard_restored true or false.
   - On failure, emit recoverable warning.
6. Return TranscriptionResult with output metadata.

### 4) Non-Fatal Output Warnings
File: `src-tauri/src/runtime.rs`

Add helper method for recoverable output-action warnings that emits `steno://error` without switching runtime phase to error.

### 5) Paste Trigger
File: `src-tauri/src/runtime.rs`

Add helper to invoke osascript command for System Events Cmd+V. Return success/failure status to output pipeline.

### 6) Tauri Command Surface
File: `src-tauri/src/lib.rs`

Add command:
- `set_clipboard_policy(policy)`

Wire command into invoke handler.

### 7) Frontend Type Surface
Files:
- `src/types.ts`
- `src/tauri.ts`

Add:
- ClipboardPolicy type.
- OutputStatus type.
- RuntimeState.clipboard_policy.
- TranscriptionResult.output_status.
- TranscriptionResult.clipboard_restored.
- New command binding `setClipboardPolicy(policy)`.

### 8) Main UI Updates
File: `src/App.tsx`

Add controls and state:
- Clipboard policy selector with two options.
- Handler to call setClipboardPolicy and refresh runtime state.

Update transcription result rendering:
- Display output status label.
- When policy is restore_previous, display clipboard restore status.

## File Change Plan
- Update `src-tauri/src/runtime.rs`.
- Update `src-tauri/src/lib.rs`.
- Update `src/types.ts`.
- Update `src/tauri.ts`.
- Update `src/App.tsx`.
- Add `spec/Steno/Text insertion and selection replacement in active app/HLD.md`.
- Add `spec/Steno/Text insertion and selection replacement in active app/LLD.md`.

## Test Plan
- Manual stop from UI triggers auto paste.
- Fn flow triggers auto paste.
- Existing selection gets replaced through normal paste semantics.
- Caret-only input inserts text.
- restore_previous policy restores clipboard when snapshot exists.
- keep_transcript policy leaves transcript in clipboard.
- Snapshot failure still allows paste attempt.
- Paste failure emits recoverable warning and still returns transcript.
- Existing runtime phase transitions remain unchanged.
- Transcription complete event includes new output metadata.

## Future Work Contract
Correction flows must not rely on arbitrary active selection. They must target insertion anchors associated with Steno output ranges. If anchors cannot be resolved, the system must fail safely without replacing unrelated text.
