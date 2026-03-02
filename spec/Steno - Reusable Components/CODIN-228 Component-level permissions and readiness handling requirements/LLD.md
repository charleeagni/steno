# LLD: CODIN-228 Component-level permissions and readiness handling requirements

## Scope
Define readiness and permission requirements so shared primitives and component-local checks can coexist.

## Readiness Contract
- Readiness response includes an aggregate status and per-check entries.
- Per-check entry includes `check_id`, `scope`, `status`, `code`, and `guidance`.
- Aggregate status enum: `ready`, `degraded`, `blocked`.

## Ownership Rules
- Shared layer provides reusable check primitives and aggregation.
- Components can contribute local checks when context is component-specific.
- No forced single owner for every permission/readiness concern.

## Evaluation Triggers
- Evaluate at startup.
- Re-evaluate on-demand via explicit refresh call.
- Re-evaluate when relevant config or permission state changes.

## Integration Plan
- Define shared readiness evaluator interface.
- Define local contributor interface for component checks.
- Standardize merge and precedence rules for aggregate status.
- Standardize actionable guidance for blocked/degraded states.

## Test Plan
- Unit tests for aggregate-status merge behavior.
- Unit tests for shared+local check composition.
- Failure-path tests for denied, unknown, and transient states.
- Regression tests for current Steno readiness messaging parity.

## Success Criteria
- Reusable primitives and local ownership are both supported.
- Outputs are implementation-ready for extraction tasks.
- Readiness behavior is deterministic and inspectable.
