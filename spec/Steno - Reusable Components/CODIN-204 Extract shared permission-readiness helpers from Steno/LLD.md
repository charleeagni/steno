# LLD: CODIN-204 Extract shared permission-readiness helpers from Steno

## Scope
Extract reusable permission and readiness helpers from Steno into shared recorder modules.

## Extraction Targets
- Permission probing and normalized permission state mapping.
- Dependency probes for recorder prerequisites.
- Readiness aggregation compatible with CODIN-228 contract.
- Actionable remediation guidance generation.

## Module Boundaries
- Shared module exports evaluators, adapters, and aggregation helpers.
- Steno retains app-specific presentation and UX strings.
- Consumers read standardized readiness outputs.

## Implementation Plan
- Isolate current Steno readiness logic and adapters.
- Move framework-neutral logic into shared package.
- Add compatibility adapter to preserve existing Steno behavior.
- Validate output shape against CODIN-228 model.

## Test Plan
- Before/after parity tests for Steno readiness outcomes.
- Unit tests for shared evaluator and aggregation logic.
- Adapter tests for permission source behavior.

## Success Criteria
- No Steno-specific assumptions leak into shared exports.
- Current Steno readiness behavior is preserved.
- Shared outputs align with CODIN-228 schema.
