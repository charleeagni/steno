export type Phase = "idle" | "recording" | "transcribing" | "error";
export type RecordMode = "push_to_talk" | "toggle";
export type MicPermission = "unknown" | "granted" | "denied";

export interface RuntimeState {
  phase: Phase;
  mode: RecordMode;
  shortcut_ready: boolean;
  mic_permission: MicPermission;
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
  model_id: string;
  duration_ms: number;
  copied_to_clipboard: boolean;
}
