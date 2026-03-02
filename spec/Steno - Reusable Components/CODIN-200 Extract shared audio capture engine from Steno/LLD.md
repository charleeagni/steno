# LLD: CODIN-200 Extract shared audio capture engine from Steno

## Scope
Extract recorder core audio capture lifecycle into a reusable engine with policy-driven output destination.

## Engine Responsibilities
- Start, stop, and finalize recording sessions.
- Manage session-scoped audio buffers.
- Produce stable output artifacts based on destination policy.

## Destination Contract
- Destination policy values: `temp`, `app_data`, `custom_path`.
- `custom_path` requires preflight validation.
- Destination resolution integrates with CODIN-229 config contract.

## Lifecycle States
- `idle`, `recording`, `stopping`, `finalizing`, `completed`, `failed`.
- Invalid state transitions are rejected with typed errors.

## Implementation Plan
- Isolate audio-capture lifecycle from current Steno runtime.
- Extract destination resolution and output finalization logic.
- Remove Steno-specific file-path assumptions from shared surface.

## Test Plan
- State transition tests for success and failure paths.
- Destination policy validation tests.
- Integration parity tests for Steno consumption path.

## Success Criteria
- Capture lifecycle is deterministic and reusable.
- Destination behavior is consumer-configurable via policy.
- No Steno-specific assumptions remain in shared contract.
