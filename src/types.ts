export type Phase = "idle" | "recording" | "transcribing" | "error";
export type RecordMode = "push_to_talk" | "toggle";
export type MicPermission = "unknown" | "granted" | "denied";
export type InputMonitoringPermission = "unknown" | "granted" | "denied";
export type ClipboardPolicy = "restore_previous" | "keep_transcript";
export type OutputStatus = "auto_pasted" | "paste_failed" | "copied_only";
export type TranscriptionRuntime = "whisper" | "parakeet" | "moonshine";
export type MoonshineVariant = "tiny" | "base";
export type ModelProfile = "fast" | "balanced" | "accurate";

export interface RuntimeState {
  phase: Phase;
  mode: RecordMode;
  shortcut_ready: boolean;
  mic_permission: MicPermission;
  input_monitoring_permission: InputMonitoringPermission;
  clipboard_policy: ClipboardPolicy;
  push_to_talk_shortcut: string;
  toggle_shortcut: string;
  runtime_selection: TranscriptionRuntime;
  model_profile: ModelProfile;
  parakeet_model_id: string;
  moonshine_variant: MoonshineVariant;
}

export interface RuntimeError {
  code: string;
  message: string;
  recoverable: boolean;
}

export interface RuntimeInitResult {
  state: RuntimeState;
  shortcut_error?: RuntimeError | null;
}

export interface TranscriptionResult {
  text: string;
  runtime_used: TranscriptionRuntime;
  model_id: string;
  duration_ms: number;
  reliability_target_ms: number;
  reliability_target_met: boolean;
  copied_to_clipboard: boolean;
  output_status: OutputStatus;
  clipboard_restored: boolean | null;
}

export type ModelDownloadStatus =
  | "not_downloaded"
  | "queued"
  | "downloading"
  | "ready"
  | "failed"
  | "canceled";

export interface ModelCatalogEntry {
  key: string;
  runtime: string;
  profile: string;
  model_id: string;
  required_files: string[];
}

export interface ModelDownloadEntry extends ModelCatalogEntry {
  status: ModelDownloadStatus;
  downloaded_bytes: number;
  total_bytes: number;
  speed_bytes_per_sec: number;
  last_error: string | null;
  updated_at_ms: number;
}

export interface ModelDownloadsSnapshot {
  models: ModelDownloadEntry[];
  queue: string[];
  active_model_key: string | null;
}

export interface InterimTranscriptionFrame {
  session_id: string;
  seq: number;
  text: string;
  is_stable: boolean;
  emitted_at_ms: number;
}

export interface LiveWordOutputFrame {
  session_id: string;
  seq: number;
  text: string;
  emitted_at_ms: number;
}
