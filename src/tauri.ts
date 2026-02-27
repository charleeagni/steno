import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ClipboardPolicy,
  InterimTranscriptionFrame,
  LiveWordOutputFrame,
  ModelDownloadsSnapshot,
  ModelProfile,
  RecordMode,
  RuntimeError,
  RuntimeInitResult,
  RuntimeState,
  TranscriptionResult,
  TranscriptionRuntime,
} from "./types";

export const commands = {
  initializeRuntime: () =>
    invoke<RuntimeInitResult>("initialize_runtime"),

  setRecordMode: (mode: RecordMode) =>
    invoke<void>("set_record_mode", { mode }),

  setClipboardPolicy: (policy: ClipboardPolicy) =>
    invoke<void>("set_clipboard_policy", { policy }),

  setHotkeyBindings: (push: string, toggle: string) =>
    invoke<void>("set_hotkey_bindings", { push, toggle }),

  captureHotkey: () =>
    invoke<string | null>("capture_hotkey"),

  setRuntimeSelection: (selection: TranscriptionRuntime) =>
    invoke<void>("set_runtime_selection", { selection }),

  setModelProfile: (profile: ModelProfile) =>
    invoke<void>("set_model_profile", { profile }),

  setParakeetModelId: (modelId: string) =>
    invoke<void>("set_parakeet_model_id", { modelId }),

  listModelDownloads: () =>
    invoke<ModelDownloadsSnapshot>("list_model_downloads"),

  startModelDownload: (modelKey: string) =>
    invoke<void>("start_model_download", { modelKey }),

  cancelModelDownload: (modelKey: string) =>
    invoke<void>("cancel_model_download", { modelKey }),

  getRecordMode: () => invoke<RecordMode>("get_record_mode"),

  startRecordingManual: () => invoke<void>("start_recording_manual"),

  stopRecordingManual: () =>
    invoke<TranscriptionResult>("stop_recording_manual"),

  getRuntimeState: () => invoke<RuntimeState>("get_runtime_state"),

  setMicPermission: (granted: boolean) =>
    invoke<void>("set_mic_permission", { granted }),

  setInputMonitoringPermission: (granted: boolean) =>
    invoke<void>("set_input_monitoring_permission", { granted }),
};

export const events = {
  onStateChanged: (cb: (state: RuntimeState) => void) =>
    listen<RuntimeState>("steno://state-changed", (event) => cb(event.payload)),

  onTranscriptionComplete: (cb: (result: TranscriptionResult) => void) =>
    listen<TranscriptionResult>("steno://transcription-complete", (event) =>
      cb(event.payload),
    ),

  onInterimTranscription: (cb: (frame: InterimTranscriptionFrame) => void) =>
    listen<InterimTranscriptionFrame>("steno://interim-transcription", (event) =>
      cb(event.payload),
    ),

  onLiveWordOutput: (cb: (frame: LiveWordOutputFrame) => void) =>
    listen<LiveWordOutputFrame>("steno://live-word-output", (event) =>
      cb(event.payload),
    ),

  onError: (cb: (error: RuntimeError) => void) =>
    listen<RuntimeError>("steno://error", (event) => cb(event.payload)),

  onModelDownloadStateChanged: (cb: (snapshot: ModelDownloadsSnapshot) => void) =>
    listen<ModelDownloadsSnapshot>("steno://model-download-state-changed", (event) =>
      cb(event.payload),
    ),
};
