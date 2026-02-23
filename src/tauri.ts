import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  RecordMode,
  RuntimeError,
  RuntimeInitResult,
  RuntimeState,
  TranscriptionResult,
} from "./types";

export const commands = {
  initializeRuntime: () =>
    invoke<RuntimeInitResult>("initialize_runtime"),

  setRecordMode: (mode: RecordMode) =>
    invoke<void>("set_record_mode", { mode }),

  getRecordMode: () => invoke<RecordMode>("get_record_mode"),

  startRecordingManual: () => invoke<void>("start_recording_manual"),

  stopRecordingManual: () =>
    invoke<TranscriptionResult>("stop_recording_manual"),

  getRuntimeState: () => invoke<RuntimeState>("get_runtime_state"),

  setMicPermission: (granted: boolean) =>
    invoke<void>("set_mic_permission", { granted }),
};

export const events = {
  onStateChanged: (cb: (state: RuntimeState) => void) =>
    listen<RuntimeState>("steno://state-changed", (event) => cb(event.payload)),

  onTranscriptionComplete: (cb: (result: TranscriptionResult) => void) =>
    listen<TranscriptionResult>("steno://transcription-complete", (event) =>
      cb(event.payload),
    ),

  onError: (cb: (error: RuntimeError) => void) =>
    listen<RuntimeError>("steno://error", (event) => cb(event.payload)),
};
