# Low Level Design: CODIN-173 Realtime Dictation Feasibility Spike

## Objective
Define implementation-ready technical increments for realtime dictation exploration without shipping unsafe active-app incremental insertion in this pass.

## Scope for This Spike
- Produce concrete engineering plan and acceptance gates for Phases A-C.
- No runtime feature implementation in this spike.
- No schema migrations or user-facing setting rollout in this spike.

## Baseline Technical Observations
- Runtime currently emits three core event types:
  - state changes
  - transcription complete
  - runtime error
- Runtime captures full recording and transcribes after stop.
- Clipboard/paste output path is commit-style and best-effort.
- There is no existing contract for interim transcript frames.

## Phase A Technical Plan: UI-Only Interim Stream

### Runtime Contract Extensions
- Add interim emission contract that carries:
  - session identifier
  - sequence number
  - partial text
  - timestamp
  - stability flag (draft/final-draft)
- Interim events are emitted only while phase is recording.

### Interim Inference Strategy
- Use periodic chunk windows during recording.
- Enforce cadence throttle to avoid runaway inference.
- Drop outdated in-flight interim tasks when newer chunk supersedes them.

### UI Behavior
- Display interim text in a dedicated preview area.
- Do not write interim text to clipboard or external app.
- On final transcription complete, replace preview with final text.

### Reliability Controls
- Add interim cadence guardrail and max concurrent interim tasks.
- Emit warning event when interim path auto-disables due budget breach.

## Phase B Technical Plan: Commit-on-Stop Hardening

### Output Action Robustness
- Keep single final output commit path.
- Add additional pre-commit focus checks where feasible.
- Preserve timeout/retry guardrails for clipboard and paste actions.

### Consistency Rules
- Final transcript supersedes any interim preview.
- If final commit fails, keep transcript in-app and emit recoverable warning.
- Never auto-apply stale interim text to active app.

## Phase C Technical Plan (Deferred): Controlled Incremental External Insertion

### Required Primitives Before Build
- Insertion anchor model tied to session and target field context.
- Anchor validation before every update.
- Rollback token to revert last applied incremental segment.

### Guarded Rollout Shape
- Opt-in per supported application family.
- Kill-switch to fall back to Phase B.
- Strict failure budget: repeated anchor failures disable incremental mode.

## Interfaces and Data Contracts

### Proposed Interim Event Payload
- session_id: string
- seq: number
- text: string
- is_stable: boolean
- emitted_at_ms: number

### Proposed Reliability Counters (In-Memory)
- interim_emit_count
- interim_drop_count
- interim_timeout_count
- interim_auto_disable_count

## Failure Modes and Handling
- Interim inference timeout:
  - discard only that interim frame
  - keep recording session active
  - increment timeout counter
- Focus changes during recording:
  - no external writes in Phase A/B interim path
- Runtime overload:
  - auto-disable interim path for session
  - surface warning to UI

## Test and Validation Matrix

### Phase A Validation
- Recording with interim enabled shows periodic preview updates.
- Final transcript replaces preview consistently.
- Interim overload triggers auto-disable and warning.

### Phase B Validation
- Final commit path unchanged functionally from user perspective.
- Output timeouts/retries remain recoverable.
- Final transcript always visible in app even when output action fails.

### Phase C Entry Criteria
- Anchor safety design approved.
- Rollback semantics proven in controlled harness.
- Failure budget policy agreed and enforced.

## Go/No-Go Gates
- Gate A (after Phase A):
  - proceed only if interim updates meet latency and stability budgets.
- Gate B (after Phase B):
  - proceed only if commit reliability remains stable or improves.
- Gate C (before Phase C):
  - proceed only with validated anchor/rollback safety and controlled rollout guardrails.

## Deliverables from This Spike
- HLD recommendation for phased rollout.
- LLD-ready interface and guardrail specification.
- Explicit defer decision for active-app incremental insertion pending safety primitives.
