# High Level Design: CODIN-173 Realtime Dictation Feasibility Spike

## Summary
This spike evaluates whether Steno can provide near-realtime dictation updates into text fields while keeping current safety and reliability expectations. Current architecture is optimized for capture-then-commit transcription, not continuous streaming insertion.

Recommendation: implement a phased rollout where early phases avoid active-app incremental insertion. Start with low-risk in-app interim feedback, then harden commit-on-stop behavior, and only then evaluate controlled active-app incremental insertion behind strict gating.

## Objectives
- Determine feasible realtime experience options within current Tauri/Rust architecture.
- Identify blockers and risk boundaries for active-app incremental updates.
- Produce a decision-ready phased path with explicit go/no-go gates.

## Current System Constraints
- Input model is hotkey-driven and session-based (`Idle -> Recording -> Transcribing -> Idle/Error`).
- Transcription occurs after stop, not continuously during capture.
- Output model writes transcript to clipboard and triggers paste into active app.
- No persistent insertion anchors exist for partial text mutation/rollback.
- No streaming ASR interface is currently integrated in runtime event contracts.
- Reliability model currently assumes single commit output, not repeated in-session edits.

## Feasibility Options and Tradeoffs

### Option 1: Chunked Local Partial Transcription During Capture
Description:
- Periodically segment captured audio and run partial transcribe passes.
- Emit intermediate text snapshots to Steno UI.

Pros:
- Reuses current local model stack.
- Keeps write operations inside app first.

Cons:
- High CPU/memory pressure from repeated decode/inference.
- Partial chunk boundaries can reduce text quality/stability.
- Requires careful throttling to avoid latency regressions.

Assessment:
- Feasible for UI preview with strict update cadence.
- Not immediately suitable for active-app insertion.

### Option 2: Near-Realtime UI-Only Preview
Description:
- Show interim draft text in Steno window while recording.
- Final transcript remains commit-on-stop to active app.

Pros:
- Lowest external side-effect risk.
- Preserves existing active-app safety model.
- Enables latency and model tuning before insertion complexity.

Cons:
- Does not yet deliver true active-field realtime dictation.
- Additional runtime events and UI states required.

Assessment:
- Strong Phase A candidate.

### Option 3: Active-App Incremental Insertion
Description:
- Continuously insert/replace partial transcript in focused external app.

Pros:
- Closest to “live dictation” user expectation.

Cons:
- Requires insertion anchoring, caret tracking, and rollback semantics.
- Significant risk of corrupting unrelated text on focus/selection drift.
- App-specific behavior differences and permission fragility.

Assessment:
- High-risk; should only follow successful UI-only and commit hardening phases.

## Recommended Phased Roadmap

### Phase A: UI-Only Interim Text
- Add runtime interim transcription stream to Steno UI only.
- No external app writes before final commit.
- Add update cadence limit and reliability metrics.

Go criteria:
- Interim updates remain within defined CPU and latency budgets.
- No increase in transcription failure rate versus current baseline.

No-go criteria:
- Sustained UI lag, thermal pressure, or large reliability regression.

### Phase B: Commit-on-Stop Hardening
- Keep external insertion single-shot on stop.
- Improve consistency checks around output action timing and final text quality.
- Add stronger handling for focus drift and output-action retries/timeouts.

Go criteria:
- Stable end-to-end success rate under repeated sessions.
- No new destructive text output incidents.

No-go criteria:
- Frequent output misplacement or unstable final commit behavior.

### Phase C: Controlled Active-App Incremental Updates (Optional)
- Introduce insertion anchors and safe rollback boundaries.
- Limit to explicitly supported/validated app contexts first.
- Keep kill-switch and fallback to Phase B behavior.

Go criteria:
- Anchor resolution succeeds reliably across target apps.
- Rollback/undo safety validated under focus changes.

No-go criteria:
- Insertion drift, selection corruption, or unbounded failure recovery.

## Key Risks and Mitigations
- Latency inflation from repeated inference:
  - Mitigate with cadence throttles and budget-based disabling.
- Text instability in partial hypotheses:
  - Mitigate by displaying drafts separately from final committed text.
- External app corruption risk:
  - Mitigate by deferring active-app incremental updates until anchor safety exists.
- Permission volatility:
  - Mitigate with startup readiness checks and soft-gated fallback paths.

## Decision
- Proceed with Phase A and Phase B only.
- Defer Phase C until anchor-based insertion and rollback guarantees are designed and validated.
