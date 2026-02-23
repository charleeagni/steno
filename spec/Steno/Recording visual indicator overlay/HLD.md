# High Level Design: CODIN-171 Recording Visual Indicator Overlay

## Summary
Add a minimal floating visual indicator that is visible while recording is active. The indicator should reflect runtime phase changes already emitted by the application state event stream, without changing transcription logic or adding waveform rendering in this work item.

## Problem Statement
The current UI already shows in-app status, but it is not sufficient when users trigger recording via global shortcut while not actively watching the main window. Users need a persistent, low-friction visual confirmation that recording is active.

## Goals
- Provide clear visual feedback when phase is `recording`.
- Keep behavior aligned with existing runtime state (`idle`, `recording`, `transcribing`, `error`).
- Reuse existing state-change events and avoid runtime pipeline changes.
- Keep implementation small and reversible.

## Non-Goals
- No waveform visualization.
- No redesign of the main Steno UI.
- No changes to recording/transcription business logic.
- No new shortcuts, settings schema, or persistence format changes.

## Scope
- In scope:
  - Add a lightweight floating overlay surface.
  - Show overlay when `recording` starts.
  - Hide or downgrade overlay when `recording` ends.
- Out of scope:
  - Rich animation system.
  - Historical recording timeline.
  - Audio level metering.

## Existing System Context
- Runtime state is already centralized in Rust (`RuntimeController`) and emitted through `steno://state-changed`.
- Frontend already subscribes to runtime state and renders text status in the main window.
- Recording transitions are already authoritative in runtime logic (`start_recording`, `stop_recording_and_transcribe`, error paths).

## Proposed Architecture

### 1) Overlay Presentation Layer
Create a dedicated overlay presentation surface (floating window) managed by the Tauri app shell.
- Small, always-on-top visual chip/badge.
- Non-interactive by default (informational only).
- Minimal styles to communicate state quickly.

### 2) Overlay State Driver
Drive overlay visibility and label from the existing runtime phase.
- `recording`: visible, high-contrast "Recording" indicator.
- `transcribing`: either hidden or neutral "Transcribing" state (final choice in LLD).
- `idle` and `error`: hidden.

### 3) Event Integration
Use existing state-change propagation as single source of truth.
- No new runtime phase definitions.
- No duplicated state machines in frontend.

## Data and Control Flow
1. User starts capture (manual button or Fn shortcut).
2. Runtime phase changes to `recording`.
3. Runtime emits `steno://state-changed`.
4. Overlay controller receives state update.
5. Overlay becomes visible with recording indicator.
6. On stop/error transition, overlay updates/hides based on mapped phase behavior.

## UX and Accessibility Principles
- Visibility first: indicator must be easy to notice.
- Low distraction: no heavy motion, no large footprint.
- Color plus text: avoid color-only communication.
- Consistent wording with main UI status labels.

## Risks and Mitigations
- Risk: Overlay lags state changes.
  - Mitigation: consume same event stream used by UI status.
- Risk: Overlay obstructs user workspace.
  - Mitigation: small default size and corner placement.
- Risk: Divergent status between overlay and main window.
  - Mitigation: runtime phase is only source of truth.

## Validation Plan
- Functional checks:
  - Start recording via manual button and Fn shortcut -> indicator appears.
  - Stop recording -> indicator hides (or transitions as defined).
  - Error transition -> indicator does not remain stuck as recording.
- Usability checks:
  - Indicator remains readable at normal desktop scale.
  - Indicator presence is noticeable within one glance.

## Acceptance Criteria
- Indicator is visible whenever runtime phase is `recording`.
- Indicator is not visible in `idle`.
- Indicator state transitions are driven by existing runtime events.
- No changes to core recording/transcription behavior.

## Future Follow-Up (Explicitly Deferred)
- Waveform and input-level visualization.
- User-configurable overlay position and opacity.
- Multi-monitor smart placement.
