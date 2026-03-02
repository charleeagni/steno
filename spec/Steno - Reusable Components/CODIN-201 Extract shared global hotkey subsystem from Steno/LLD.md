# LLD: CODIN-201 Extract shared global hotkey subsystem from Steno

## Scope
Extract a reusable hotkey backend for registration, invocation, and conflict reporting.

## Contract Inputs
- Fixed binding fields: `push_to_talk` and `toggle`.
- Consumer UI may pre-validate before submit.
- Shared backend enforces final normalization and conflict checks.

## Backend Responsibilities
- Register/unregister global shortcuts.
- Route invocation events to recorder actions.
- Emit structured failure codes and recovery guidance.

## Error Model
- `shortcut_conflict`, `invalid_shortcut`, `reserved_shortcut`, `shortcut_init_failed`.
- Error payload includes user-actionable guidance.

## Implementation Plan
- Extract hotkey manager and callback wiring from Steno runtime.
- Split UI-originated values from backend registration concerns.
- Preserve existing timeout and reinitialize behavior.

## Test Plan
- Registration lifecycle tests.
- Validation and conflict tests for fixed push/toggle bindings.
- Invocation routing tests for both action modes.

## Success Criteria
- Backend is reusable across consumer apps.
- Consumer remains owner of hotkey UI.
- Validation authority boundaries remain explicit.
