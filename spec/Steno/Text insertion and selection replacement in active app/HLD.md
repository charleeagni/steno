# High Level Design: CODIN-172 Text Insertion and Selection Replacement in Active App

## Summary
Implement a single output action that auto-pastes the latest transcript into the active app after transcription completes. Add clipboard policy controls so users can either restore previous clipboard contents or keep the transcript in clipboard. Do not implement correction-time reselection in this ticket; define a clear contract for that future work.

## Problem Statement
The current flow only copies transcript text to clipboard. Users need a low-friction path that inserts dictated text directly into the active app. At the same time, clipboard ownership must remain predictable for users who rely on clipboard history and copy/paste workflows.

## Goals
- Auto-insert transcript into the frontmost app using standard paste semantics.
- Support two clipboard policies:
  - Restore previous clipboard.
  - Keep transcript in clipboard.
- Keep transcription completion non-blocking even when output actions fail.
- Expose output action status to UI.
- Define future correction semantics for replacing only Steno-inserted text.

## Non-Goals
- No separate replace command.
- No app-specific text automation.
- No persistent settings schema changes.
- No correction workflow implementation.

## Scope
- In scope:
  - Runtime output pipeline changes after transcription.
  - Clipboard policy in runtime state and UI.
  - Output result metadata for observability.
  - Design contract for future text reselection.
- Out of scope:
  - Accessibility text-range writes.
  - Selection-anchor persistence.
  - Multi-step correction UX.

## Existing System Context
- Runtime currently transcribes audio and writes transcript to clipboard.
- UI listens for transcription completion and renders latest transcript.
- macOS accessibility permissions are already part of app readiness flow.

## Proposed Architecture

### 1) Output Action Pipeline
After transcript text is finalized:
1. Optionally snapshot current clipboard text.
2. Write transcript to clipboard.
3. Trigger standard paste shortcut in active app.
4. Optionally restore previous clipboard.
5. Emit transcription completion event with output metadata.

### 2) Clipboard Policy
Add clipboard policy to runtime state:
- `restore_previous` (default): attempt clipboard snapshot and restore.
- `keep_transcript`: do not restore previous clipboard.

### 3) Non-Fatal Error Behavior
Clipboard/paste failures are emitted as recoverable runtime errors. Transcription completion still returns transcript text and output metadata.

### 4) UI Controls
Expose clipboard policy selector in main controls panel and show lightweight output result labels.

## Future Selection Semantics Contract
For future correction flows, replacement must target Steno-inserted ranges instead of arbitrary active selections.

- Principle:
  - Replace only known Steno insertion anchors.
- Preferred mechanism:
  - AX-based focused element text-range APIs.
- Fallback:
  - If anchor cannot be resolved, fail safely and do not perform blind replacement.

## Risks and Mitigations
- Risk: Paste command blocked by permissions or app restrictions.
  - Mitigation: recoverable error emission; keep transcript completion successful.
- Risk: Clipboard restore may fail.
  - Mitigation: emit recoverable error and return explicit restore status.
- Risk: Output action races with user focus changes.
  - Mitigation: keep operation short and status-visible; accept best-effort semantics for v1.

## Acceptance Criteria
- Transcript is auto-pasted on completion when clipboard write succeeds.
- Clipboard policy is user-selectable at runtime.
- `restore_previous` attempts restoration and reports restore outcome.
- Output status is emitted and rendered.
- Transcription completion remains successful even when paste/restore fails.
- Future correction semantics are documented with explicit anchor-based contract.
