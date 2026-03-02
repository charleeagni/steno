# LLD: CODIN-196 Steno - Reusable Components

## Scope
Provide low-level design coverage for all confirmed CODIN-196 sub-tasks, with the parent-level constraint that live transcript functionality is excluded from this planning pass.

## Scope Decision Applied
- Live transcript features are out of scope for CODIN-196 planning.
- CODIN-203 remains listed for traceability but is deferred in this pass.

## Sub-Task LLD Index
- CODIN-229: [Define recorder plugin configuration contract and consumer override points](./CODIN-229%20Define%20recorder%20plugin%20configuration%20contract%20and%20consumer%20override%20points/LLD.md)
- CODIN-228: [Component-level permissions and readiness handling requirements](./CODIN-228%20Component-level%20permissions%20and%20readiness%20handling%20requirements/LLD.md)
- CODIN-227: [Recorder overlay ownership and live transcript injection requirements](./CODIN-227%20Recorder%20overlay%20ownership%20and%20live%20transcript%20injection%20requirements/LLD.md)
- CODIN-204: [Extract shared permission-readiness helpers from Steno](./CODIN-204%20Extract%20shared%20permission-readiness%20helpers%20from%20Steno/LLD.md)
- CODIN-203: [Extract shared interim-live transcript pipeline from Steno](./CODIN-203%20Extract%20shared%20interim-live%20transcript%20pipeline%20from%20Steno/LLD.md)
- CODIN-202: [Extract shared overlay runtime module from Steno](./CODIN-202%20Extract%20shared%20overlay%20runtime%20module%20from%20Steno/LLD.md)
- CODIN-201: [Extract shared global hotkey subsystem from Steno](./CODIN-201%20Extract%20shared%20global%20hotkey%20subsystem%20from%20Steno/LLD.md)
- CODIN-200: [Extract shared audio capture engine from Steno](./CODIN-200%20Extract%20shared%20audio%20capture%20engine%20from%20Steno/LLD.md)

## Composition Order
1. CODIN-229 defines shared consumer-facing configuration boundaries.
2. CODIN-228 defines readiness/permission result model and ownership rules.
3. CODIN-200, CODIN-201, CODIN-202, CODIN-204 consume the contract decisions.
4. CODIN-227 finalizes overlay ownership and override boundaries.
5. CODIN-203 is deferred and not implemented in this parent pass.

## Cross-Cutting Contracts
- Consumer-owned settings UI, shared backend validation and registration.
- Recorder-owned overlay shell with consumer content-slot override.
- Deterministic audio capture lifecycle with policy-driven output destination.
- Explicit readiness outcomes with actionable guidance.

## Delivery Notes
- Each sub-task has an independent LLD for implementation planning.
- CODIN-203 is intentionally deferred due parent scope cut.
- Plane status for each listed task is updated to `LLD` in this pass.
