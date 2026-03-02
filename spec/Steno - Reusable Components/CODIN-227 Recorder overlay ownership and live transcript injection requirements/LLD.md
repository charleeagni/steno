# LLD: CODIN-227 Recorder overlay ownership and live transcript injection requirements

## Scope
Define recorder-owned overlay lifecycle and consumer override boundaries.

## Explicit Scope Cut
- Live transcript injection requirements are excluded from this task in CODIN-196.

## Ownership Contract
- Recorder plugin owns overlay window/surface lifecycle.
- Recorder plugin renders required default recording indicator.
- Consumer can provide content only inside a reserved content slot.
- Content slot provider is bound at initialization for this phase.

## Rendering States
- Required shell states: `idle`, `recording`, `processing`, `error`.
- Default indicator is always available in `recording` state.
- Consumer slot content cannot take ownership of shell lifecycle.

## File-Level Plan
- Extract overlay shell lifecycle management from Steno runtime.
- Define slot-render interface for consumer-provided content.
- Keep default indicator in shared shell renderer.
- Remove transcript-coupled logic from this task scope.

## Test Plan
- Overlay lifecycle tests for show, hide, and teardown.
- Rendering tests for default shell + slot content.
- Boundary tests ensuring slot renderer cannot break shell control.

## Success Criteria
- Ownership and override points are explicit.
- Default and custom rendering paths are both supported.
- No live transcript dependency remains in this task scope.
