# CODIN-173 Spike Findings

## Decision Summary
- Realtime dictation is feasible in this codebase only through phased rollout.
- Safe immediate path is UI-only interim updates during recording.
- Active-app incremental insertion is deferred due high corruption risk without anchors.

## What We Learned
- Current runtime is capture-then-commit, not streaming.
- Existing clipboard/paste path is single-shot and best-effort.
- There is no insertion anchor model for partial external updates.
- Continuous external insertion would require anchor validation and rollback semantics.

## Recommended Path
1. Phase A: UI-only interim text stream while recording.
2. Phase B: Harden final commit-on-stop reliability.
3. Phase C (optional): Controlled incremental external insertion only after anchor safety.

## Go / No-Go Gates
- Gate A: Proceed only if interim updates stay within latency and CPU budgets.
- Gate B: Proceed only if final commit reliability remains stable or improves.
- Gate C: Proceed only with validated anchor + rollback safety.

## Risks to Track
- Inference cadence causing thermal/latency regressions.
- Partial hypothesis instability confusing users.
- Focus/selection drift in external apps during incremental updates.

## Explicit Deferral
- Do not implement active-app incremental insertion until anchor and rollback primitives are implemented and validated.

## References
- `spec/Steno/Realtime dictation feasibility spike/HLD.md`
- `spec/Steno/Realtime dictation feasibility spike/LLD.md`
