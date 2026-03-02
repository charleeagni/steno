# LLD: CODIN-229 Define recorder plugin configuration contract and consumer override points

## Scope
Define a shared recorder plugin configuration contract for Steno, Personal, and future Tauri apps.

## Explicit Scope Cut
- Live transcript-related configuration is excluded from this task.

## Contract Decisions
- One canonical `RecorderPluginConfig` object is the source of truth.
- Runtime updates are handled through an atomic `RecorderConfigPatch` flow.
- Any invalid field rejects the full patch; no partial application.

## Configuration Model
- `destination_policy`: `temp`, `app_data`, `custom_path`.
- `custom_output_path`: optional, required only when `destination_policy=custom_path`.
- `hotkeys.push_to_talk`: required shortcut string.
- `hotkeys.toggle`: required shortcut string.
- `overlay.content_slot_provider`: init-time registration only.

## Ownership Boundaries
- Consumer owns hotkey settings UI and submitted values.
- Shared backend owns shortcut normalization, conflict checks, and registration.
- Consumer can pre-validate, but backend is final authority.

## Validation Rules
- Destination policy and path combination must be valid.
- `custom_path` must resolve and be writable before recording starts.
- `push_to_talk` and `toggle` must be distinct and non-reserved.
- Overlay slot provider must satisfy renderer interface at init-time.

## File-Level Plan
- Define shared config schema/types for config and patch payloads.
- Add validator module with stable error codes.
- Add adapter mapping from current Steno `settings.json` fields.
- Preserve current Steno defaults through schema defaults.

## Test Plan
- Unit tests for config schema and patch atomicity.
- Unit tests for destination policy and path validation.
- Unit tests for shortcut normalization/conflict validation.
- Compatibility tests for existing Steno persisted settings.

## Success Criteria
- Contract is app-agnostic and explicit.
- Runtime update behavior is deterministic and safe.
- Shared backend and consumer responsibilities are unambiguous.
