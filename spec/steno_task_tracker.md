# Steno Task Tracker

## Modules

| Plane Key | Title | Description |
|---|---|---|
| CODIN-164 | Module: Always-On Desktop Runtime | Planning bucket for making Steno continuously available from anywhere in macOS, with robust background behavior and clear operational states. |
| CODIN-165 | Module: Input and Recording UX | Planning bucket for user input ergonomics: customizable shortcuts, recording-state visual feedback, and minimal-friction interaction modes. |
| CODIN-166 | Module: Transcription Runtime and Models | Planning bucket for engine/runtime support, including Whisper and Parakeet model options and runtime selection behavior. |
| CODIN-167 | Module: Reliability and Settings Experience | Planning bucket for latency/reliability guardrails and a clean, usable settings UI for daily operation. |

## Work Items

| Plane Key | Title | Description |
|---|---|---|
| CODIN-168 | Background run mode (menubar/tray + headless hotkey capture) | Allow Steno to remain available without foreground window focus. Define behavior for app launch, hide/restore, and persistent global shortcut readiness. |
| CODIN-169 | Startup and permission readiness checks | At startup, validate microphone and required macOS permissions for global operation. Surface actionable remediation when blocked. |
| CODIN-170 | Customizable hotkeys for push-to-talk and toggle modes | Add settings to configure independent shortcuts for push-to-talk and toggle recording. Persist config and validate conflicts/reserved combos. |
| CODIN-171 | Recording visual indicator overlay | Provide clear in-app or floating visual feedback that recording is active. Start with minimal indicator; waveform can be optional follow-up. |
| CODIN-172 | Text insertion and selection replacement in active app | Add controlled output actions: paste transcript at cursor and replace selected text in focused app where permissions allow. |
| CODIN-173 | Realtime dictation feasibility spike | Investigate viability and constraints of near-realtime transcription updates into text fields. Deliver recommendation and phased implementation path. |
| CODIN-174 | [Transcription Runtime and Models] Parakeet runtime/model support in transcriber module | Add Parakeet model loading/inference path alongside existing Whisper support with clear runtime selection contract. |
| CODIN-175 | [Transcription Runtime and Models] Model profile defaults and runtime selector UX | Define default model profiles (fast/balanced/accurate), expose selection in settings, and persist user choice. |
| CODIN-176 | [Reliability and Settings Experience] Latency and reliability guardrails | Define and enforce reliability targets and graceful failure paths for recording, transcription, and clipboard/output actions. |
| CODIN-177 | [Reliability and Settings Experience] Settings UI redesign for clarity | Design and implement a clean settings experience covering hotkeys, mode selection, runtime/model selection, and output behavior. |

## Suggested Execution Order

1. CODIN-168
2. CODIN-170
3. CODIN-171
4. CODIN-176
5. CODIN-177
6. CODIN-174

