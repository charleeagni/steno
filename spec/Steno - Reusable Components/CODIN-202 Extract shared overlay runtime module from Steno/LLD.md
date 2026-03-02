# LLD: CODIN-202 Extract shared overlay runtime module from Steno

## Scope
Extract Steno overlay runtime into a reusable recorder-owned module with consumer content-slot override support.

## Explicit Scope Cut
- Live transcript integration is excluded from this task in CODIN-196.

## Runtime Responsibilities
- Own overlay lifecycle creation, show, hide, and teardown.
- Render default recording indicator state.
- Expose content slot for consumer-provided rendering.

## Public Surface
- `OverlayRuntimeController` lifecycle interface.
- `OverlayRenderContext` for shell and slot rendering.
- `OverlayContentSlot` callback contract bound at init-time.

## Boundary Rules
- Shared module controls shell lifecycle and state transitions.
- Consumer controls only content inside allocated slot.
- Overlay runtime remains independent of app-specific settings UI.

## Implementation Plan
- Extract window/surface lifecycle code from Steno.
- Move default overlay indicator into shared shell renderer.
- Define slot interface for consumer content injection.
- Preserve current Steno behavior via adapter wiring.

## Test Plan
- Lifecycle tests for overlay shell operations.
- Rendering tests for default shell and slot content.
- Regression tests against current Steno overlay behavior.

## Success Criteria
- Shared overlay shell is reusable and stable.
- Consumer override boundary is constrained and clear.
- No live transcript dependency is introduced.
