import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkInputMonitoringPermission,
  checkMicrophonePermission,
  requestAccessibilityPermission,
  requestInputMonitoringPermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { commands, events } from "./tauri";
import type {
  ClipboardPolicy,
  InterimTranscriptionFrame,
  LiveWordOutputFrame,
  ModelDownloadEntry,
  ModelDownloadsSnapshot,
  ModelDownloadStatus,
  ModelProfile,
  OutputStatus,
  RuntimeError,
  RuntimeInitResult,
  RuntimeState,
  TranscriptionResult,
  TranscriptionRuntime,
} from "./types";

type SettingsSectionId =
  | "overview"
  | "input"
  | "transcription"
  | "models"
  | "transcript"
  | "diagnostics";

type SettingsSectionTone = "neutral" | "warning" | "danger";

type SettingsSection = {
  id: SettingsSectionId;
  title: string;
  description: string;
  badge?: string;
  tone?: SettingsSectionTone;
  content: ReactNode;
};

type ShortcutCaptureTarget = "push" | "toggle";
const DEFAULT_PARAKEET_MODEL_ID = "istupakov/parakeet-tdt-0.6b-v3-onnx";

const initialState: RuntimeState = {
  phase: "idle",
  mode: "push_to_talk",
  shortcut_ready: false,
  mic_permission: "unknown",
  input_monitoring_permission: "unknown",
  clipboard_policy: "restore_previous",
  push_to_talk_shortcut: "Fn",
  toggle_shortcut: "Shift+Fn",
  runtime_selection: "whisper",
  model_profile: "balanced",
  parakeet_model_id: DEFAULT_PARAKEET_MODEL_ID,
  moonshine_variant: "tiny",
};

const STARTUP_TIMEOUT_MS = 8000;
const LISTENER_TIMEOUT_MS = 3000;
const STARTUP_WATCHDOG_MS = 15000;
const MB_DIVISOR = 1024 * 1024;

const emptyModelDownloads: ModelDownloadsSnapshot = {
  models: [],
  queue: [],
  active_model_key: null,
};

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined;

  const timeoutPromise = new Promise<never>((_, reject) => {
    timeoutId = setTimeout(() => {
      reject(new Error(`${label} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
  });

  try {
    return await Promise.race([promise, timeoutPromise]);
  } finally {
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
  }
}

function formatModelStatus(status: ModelDownloadStatus): string {
  if (status === "not_downloaded") return "Not downloaded";
  if (status === "queued") return "Queued";
  if (status === "downloading") return "Downloading";
  if (status === "ready") return "Ready";
  if (status === "failed") return "Failed";
  return "Canceled";
}

function formatModelMegabytes(bytes: number): string {
  return `${(bytes / MB_DIVISOR).toFixed(1)} MB`;
}

function formatModelSpeed(speedBytesPerSecond: number): string {
  if (speedBytesPerSecond <= 0) return "0.0 MB/s";
  return `${(speedBytesPerSecond / MB_DIVISOR).toFixed(1)} MB/s`;
}

function modelProgressLabel(model: ModelDownloadEntry): string {
  const downloaded = formatModelMegabytes(model.downloaded_bytes);
  const total = model.total_bytes > 0 ? formatModelMegabytes(model.total_bytes) : "Unknown";
  const speed = formatModelSpeed(model.speed_bytes_per_sec);
  if (model.status === "ready") {
    return `Ready (${downloaded})`;
  }
  return `${downloaded} / ${total} (${speed})`;
}

function App() {
  const [isMac, setIsMac] = useState(false);
  const [runtimeState, setRuntimeState] = useState<RuntimeState>(initialState);
  const [latestTranscript, setLatestTranscript] = useState("");
  const [interimTranscript, setInterimTranscript] = useState("");
  const [liveWordTranscript, setLiveWordTranscript] = useState("");
  const [outputStatus, setOutputStatus] = useState<"idle" | OutputStatus>("idle");
  const [clipboardRestored, setClipboardRestored] = useState<boolean | null>(null);
  const [runtimeError, setRuntimeError] = useState<RuntimeError | null>(null);
  const [loading, setLoading] = useState(true);
  const [needsAccessibilityPermission, setNeedsAccessibilityPermission] = useState(false);
  const [needsMicPermission, setNeedsMicPermission] = useState(false);
  const [needsInputMonitoringPermission, setNeedsInputMonitoringPermission] = useState(false);
  const [shortcutInitError, setShortcutInitError] = useState<RuntimeError | null>(null);
  const [pushShortcutDraft, setPushShortcutDraft] = useState(initialState.push_to_talk_shortcut);
  const [toggleShortcutDraft, setToggleShortcutDraft] = useState(initialState.toggle_shortcut);
  const [shortcutCaptureTarget, setShortcutCaptureTarget] = useState<ShortcutCaptureTarget | null>(
    null,
  );
  const [runtimeUsed, setRuntimeUsed] = useState<TranscriptionRuntime | null>(null);
  const [modelUsed, setModelUsed] = useState<string>("");
  const [durationMs, setDurationMs] = useState<number | null>(null);
  const [latencyTargetMs, setLatencyTargetMs] = useState<number | null>(null);
  const [latencyTargetMet, setLatencyTargetMet] = useState<boolean | null>(null);
  const [lastReliabilityWarning, setLastReliabilityWarning] = useState<string | null>(null);
  const [modelDownloads, setModelDownloads] =
    useState<ModelDownloadsSnapshot>(emptyModelDownloads);
  const [activeSectionId, setActiveSectionId] = useState<SettingsSectionId>("overview");
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const runtimePhaseRef = useRef<RuntimeState["phase"]>(initialState.phase);
  const interimSessionRef = useRef<string | null>(null);
  const interimSeqRef = useRef(0);
  const liveWordSessionRef = useRef<string | null>(null);
  const liveWordSeqRef = useRef(0);

  const syncAccessibilityPermission = async () => {
    const granted = await withTimeout(
      checkAccessibilityPermission(),
      STARTUP_TIMEOUT_MS,
      "Accessibility permission check",
    );
    setNeedsAccessibilityPermission(!granted);
    return granted;
  };

  const syncMicPermission = async () => {
    const granted = await withTimeout(
      checkMicrophonePermission(),
      STARTUP_TIMEOUT_MS,
      "Microphone permission check",
    );
    await commands.setMicPermission(granted);
    setNeedsMicPermission(!granted);
    return granted;
  };

  const syncInputMonitoringPermission = async () => {
    const granted = await withTimeout(
      checkInputMonitoringPermission(),
      STARTUP_TIMEOUT_MS,
      "Input Monitoring permission check",
    );
    await commands.setInputMonitoringPermission(granted);
    setNeedsInputMonitoringPermission(!granted);
    return granted;
  };

  useEffect(() => {
    let disposed = false;
    let startupSettled = false;
    let startupWatchdog: ReturnType<typeof setTimeout> | undefined;
    let unlistenState: (() => void) | undefined;
    let unlistenTranscription: (() => void) | undefined;
    let unlistenInterim: (() => void) | undefined;
    let unlistenLiveWord: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let unlistenModelDownloads: (() => void) | undefined;

    const reportStartupFailure = (message: string, code = "startup_failed") => {
      const startupError: RuntimeError = {
        code,
        message,
        recoverable: true,
      };
      if (!disposed) {
        setRuntimeError(startupError);
        setShortcutInitError(startupError);
      }
    };

    const registerListeners = async () => {
      try {
        const stateUnlisten = await withTimeout(
          events.onStateChanged((state) => {
            runtimePhaseRef.current = state.phase;
            setRuntimeState(state);
          }),
          LISTENER_TIMEOUT_MS,
          "State listener registration",
        );

        const transcriptionUnlisten = await withTimeout(
          events.onTranscriptionComplete((result: TranscriptionResult) => {
            setLatestTranscript(result.text);
            setOutputStatus(result.output_status);
            setClipboardRestored(result.clipboard_restored);
            setRuntimeUsed(result.runtime_used);
            setModelUsed(result.model_id);
            setDurationMs(result.duration_ms);
            setLatencyTargetMs(result.reliability_target_ms);
            setLatencyTargetMet(result.reliability_target_met);
            setInterimTranscript(result.text);
            interimSessionRef.current = null;
            interimSeqRef.current = 0;
            setRuntimeError(null);
          }),
          LISTENER_TIMEOUT_MS,
          "Transcription listener registration",
        );

        const interimUnlisten = await withTimeout(
          events.onInterimTranscription((frame: InterimTranscriptionFrame) => {
            if (runtimePhaseRef.current !== "recording") {
              return;
            }

            if (interimSessionRef.current !== frame.session_id) {
              interimSessionRef.current = frame.session_id;
              interimSeqRef.current = 0;
            }

            if (frame.seq < interimSeqRef.current) {
              return;
            }

            interimSeqRef.current = frame.seq;
            setInterimTranscript(frame.text);
          }),
          LISTENER_TIMEOUT_MS,
          "Interim listener registration",
        );

        const liveWordUnlisten = await withTimeout(
          events.onLiveWordOutput((frame: LiveWordOutputFrame) => {
            if (runtimePhaseRef.current !== "recording") {
              return;
            }

            if (liveWordSessionRef.current !== frame.session_id) {
              liveWordSessionRef.current = frame.session_id;
              liveWordSeqRef.current = 0;
            }

            if (frame.seq < liveWordSeqRef.current) {
              return;
            }

            liveWordSeqRef.current = frame.seq;
            setLiveWordTranscript(frame.text);
          }),
          LISTENER_TIMEOUT_MS,
          "Live word listener registration",
        );

        const errorUnlisten = await withTimeout(
          events.onError((error) => {
            setRuntimeError(error);
            if (isReliabilityWarningCode(error.code)) {
              setLastReliabilityWarning(error.message);
            }
          }),
          LISTENER_TIMEOUT_MS,
          "Error listener registration",
        );

        const modelDownloadsUnlisten = await withTimeout(
          events.onModelDownloadStateChanged((snapshot) => {
            setModelDownloads(snapshot);
          }),
          LISTENER_TIMEOUT_MS,
          "Model download listener registration",
        );

        const initialModelSnapshot = await withTimeout(
          commands.listModelDownloads(),
          LISTENER_TIMEOUT_MS,
          "Model snapshot fetch",
        );
        setModelDownloads(initialModelSnapshot);

        if (disposed) {
          stateUnlisten();
          transcriptionUnlisten();
          interimUnlisten();
          liveWordUnlisten();
          errorUnlisten();
          modelDownloadsUnlisten();
          return;
        }

        unlistenState = stateUnlisten;
        unlistenTranscription = transcriptionUnlisten;
        unlistenInterim = interimUnlisten;
        unlistenLiveWord = liveWordUnlisten;
        unlistenError = errorUnlisten;
        unlistenModelDownloads = modelDownloadsUnlisten;
      } catch (error) {
        const message =
          error instanceof Error
            ? error.message
            : "Failed to register runtime event listeners.";
        reportStartupFailure(message, "listener_setup_failed");
      }
    };

    const init = async () => {
      let osName = "unknown";
      try {
        osName = String(await platform()).toLowerCase();
      } catch (error) {
        // Browser/dev fallback when Tauri plugins are unavailable.

        const navPlatform = typeof navigator !== "undefined" ? navigator.platform : "";
        const navUserAgent = typeof navigator !== "undefined" ? navigator.userAgent : "";
        const fallbackIsMac =
          /mac/i.test(navPlatform) || /mac os x|macintosh/i.test(navUserAgent);
        osName = fallbackIsMac ? "macos" : "unknown";

        if (!fallbackIsMac) {
          reportStartupFailure(
            error instanceof Error ? error.message : "Failed to detect platform.",
            "platform_detect_failed",
          );
        }
      }

      const mac = osName === "macos" || osName === "darwin";
      if (!disposed) {
        setIsMac(mac);
      }

      if (!mac) {
        if (!disposed) {
          setLoading(false);
          setRuntimeError({
            code: "platform_not_supported",
            message: "Steno currently supports macOS only.",
            recoverable: false,
          });
        }
        startupSettled = true;
        return;
      }

      void registerListeners();

      try {
        const hasAccessibility = await syncAccessibilityPermission();
        if (!hasAccessibility) {
          startupSettled = true;
          return;
        }

        const hasMic = await syncMicPermission();

        if (!hasMic) {
          if (!disposed) {
            setNeedsMicPermission(true);
          }
          startupSettled = true;
          return;
        }

        await syncInputMonitoringPermission();

        const initResult = await withTimeout(
          commands.initializeRuntime(),
          STARTUP_TIMEOUT_MS,
          "Runtime initialization",
        );
        if (!disposed) {
          applyInitResult(initResult);
        }
      } catch (error) {
        reportStartupFailure(
          error instanceof Error ? error.message : "Failed to initialize Steno runtime.",
        );
      } finally {
        startupSettled = true;
        if (!disposed) {
          setLoading(false);
        }
      }
    };

    startupWatchdog = setTimeout(() => {
      if (!startupSettled && !disposed) {
        reportStartupFailure(
          "Startup watchdog triggered. Initialization stalled; check macOS permissions and retry.",
          "startup_watchdog_timeout",
        );
        setLoading(false);
      }
    }, STARTUP_WATCHDOG_MS);

    void init();

    return () => {
      disposed = true;
      if (startupWatchdog) {
        clearTimeout(startupWatchdog);
      }
      unlistenState?.();
      unlistenTranscription?.();
      unlistenInterim?.();
      unlistenLiveWord?.();
      unlistenError?.();
      unlistenModelDownloads?.();
    };
  }, []);

  useEffect(() => {
    if (runtimeState.phase === "recording") {
      setInterimTranscript("");
      setLiveWordTranscript("");
      interimSessionRef.current = null;
      interimSeqRef.current = 0;
      liveWordSessionRef.current = null;
      liveWordSeqRef.current = 0;
    }

    if (runtimeState.phase === "idle") {
      interimSessionRef.current = null;
      interimSeqRef.current = 0;
      liveWordSessionRef.current = null;
      liveWordSeqRef.current = 0;
    }
  }, [runtimeState.phase]);

  useEffect(() => {
    if (!isMac || !needsAccessibilityPermission) {
      return;
    }

    const onFocus = () => {
      void syncAccessibilityPermission();
    };

    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
    };
  }, [isMac, needsAccessibilityPermission]);

  useEffect(() => {
    if (!isMac || !needsMicPermission) {
      return;
    }

    const onFocus = () => {
      void syncMicPermission();
    };

    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
    };
  }, [isMac, needsMicPermission]);

  useEffect(() => {
    if (!isMac || !needsInputMonitoringPermission) {
      return;
    }

    const onFocus = () => {
      void (async () => {
        try {
          const granted = await syncInputMonitoringPermission();
          if (!granted) {
            return;
          }

          const initResult = await withTimeout(
            commands.initializeRuntime(),
            STARTUP_TIMEOUT_MS,
            "Runtime initialization",
          );
          applyInitResult(initResult);
          setRuntimeError(null);
        } catch (error) {
          setRuntimeError({
            code: "input_monitoring_permission_check_failed",
            message:
              error instanceof Error
                ? error.message
                : "Unable to refresh Input Monitoring permission state.",
            recoverable: true,
          });
        }
      })();
    };

    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
    };
  }, [isMac, needsInputMonitoringPermission]);

  useEffect(() => {
    setPushShortcutDraft(runtimeState.push_to_talk_shortcut);
    setToggleShortcutDraft(runtimeState.toggle_shortcut);
  }, [runtimeState.push_to_talk_shortcut, runtimeState.toggle_shortcut]);

  const applyInitResult = (initResult: RuntimeInitResult) => {
    setRuntimeState(initResult.state);
    setShortcutInitError(initResult.shortcut_error ?? null);
  };

  const grantMicPermission = async () => {
    try {
      await withTimeout(
        requestMicrophonePermission(),
        STARTUP_TIMEOUT_MS,
        "Microphone permission request",
      );
      const granted = await syncMicPermission();

      if (granted) {
        const initResult = await withTimeout(
          commands.initializeRuntime(),
          STARTUP_TIMEOUT_MS,
          "Runtime initialization",
        );
        applyInitResult(initResult);
        setRuntimeError(null);
      } else {
        setRuntimeError({
          code: "mic_permission_still_denied",
          message:
            "Microphone is still not granted to Steno. In macOS Settings > Privacy & Security > Microphone, enable Steno, then return to the app.",
          recoverable: true,
        });
      }
    } catch (error) {
      setRuntimeError({
        code: "mic_permission_request_failed",
        message:
          error instanceof Error ? error.message : "Unable to request microphone permission.",
        recoverable: true,
      });
    }
  };

  const grantAccessibilityPermission = async () => {
    try {
      await withTimeout(
        requestAccessibilityPermission(),
        STARTUP_TIMEOUT_MS,
        "Accessibility permission request",
      );
      const hasAccessibility = await syncAccessibilityPermission();

      if (!hasAccessibility) {
        setRuntimeError({
          code: "accessibility_permission_still_denied",
          message:
            "Accessibility is still not granted to Steno. In macOS Settings > Privacy & Security > Accessibility, enable Steno, then return to the app.",
          recoverable: true,
        });
        return;
      }

      const hasMic = await syncMicPermission();
      if (!hasMic) {
        setNeedsMicPermission(true);
        setRuntimeError(null);
        return;
      }

      const initResult = await withTimeout(
        commands.initializeRuntime(),
        STARTUP_TIMEOUT_MS,
        "Runtime initialization",
      );
      applyInitResult(initResult);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "accessibility_permission_request_failed",
        message:
          error instanceof Error ? error.message : "Unable to request accessibility permission.",
        recoverable: true,
      });
    }
  };

  const grantInputMonitoringPermission = async () => {
    try {
      await withTimeout(
        requestInputMonitoringPermission(),
        STARTUP_TIMEOUT_MS,
        "Input Monitoring permission request",
      );

      const granted = await syncInputMonitoringPermission();
      if (!granted) {
        setRuntimeError({
          code: "input_monitoring_permission_still_denied",
          message:
            "Input Monitoring is still not granted to Steno. In macOS Settings > Privacy & Security > Input Monitoring, enable Steno, then return to the app.",
          recoverable: true,
        });
        return;
      }

      const initResult = await withTimeout(
        commands.initializeRuntime(),
        STARTUP_TIMEOUT_MS,
        "Runtime initialization",
      );
      applyInitResult(initResult);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "input_monitoring_permission_request_failed",
        message:
          error instanceof Error
            ? error.message
            : "Unable to request Input Monitoring permission.",
        recoverable: true,
      });
    }
  };

  const onShortcutCaptureToggle = async (target: ShortcutCaptureTarget) => {
    if (shortcutCaptureTarget) {
      return;
    }

    setShortcutCaptureTarget(target);
    try {
      const capturedShortcut = await commands.captureHotkey();
      if (!capturedShortcut) {
        return;
      }

      if (target === "push") {
        setPushShortcutDraft(capturedShortcut);
      } else {
        setToggleShortcutDraft(capturedShortcut);
      }
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "capture_hotkey_failed",
        message: error instanceof Error ? error.message : "Failed to capture shortcut.",
        recoverable: true,
      });
    } finally {
      setShortcutCaptureTarget(null);
    }
  };

  const onClipboardPolicyChange = async (policy: ClipboardPolicy) => {
    try {
      await commands.setClipboardPolicy(policy);
      const nextState = await commands.getRuntimeState();
      setRuntimeState(nextState);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "set_clipboard_policy_failed",
        message:
          error instanceof Error ? error.message : "Failed to update clipboard policy.",
        recoverable: true,
      });
    }
  };

  const onRuntimeSelectionChange = async (selection: TranscriptionRuntime) => {
    try {
      await commands.setRuntimeSelection(selection);
      const nextState = await commands.getRuntimeState();
      setRuntimeState(nextState);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "set_runtime_selection_failed",
        message:
          error instanceof Error ? error.message : "Failed to update transcription runtime.",
        recoverable: true,
      });
    }
  };

  const onModelProfileChange = async (profile: ModelProfile) => {
    try {
      await commands.setModelProfile(profile);
      const nextState = await commands.getRuntimeState();
      setRuntimeState(nextState);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "set_model_profile_failed",
        message: error instanceof Error ? error.message : "Failed to update model profile.",
        recoverable: true,
      });
    }
  };

  const onParakeetModelChange = async (modelId: string) => {
    try {
      await commands.setParakeetModelId(modelId);
      const nextState = await commands.getRuntimeState();
      setRuntimeState(nextState);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "set_parakeet_model_failed",
        message: error instanceof Error ? error.message : "Failed to update Parakeet model.",
        recoverable: true,
      });
    }
  };

  const onMoonshineVariantChange = async (variant: string) => {
    try {
      // @ts-expect-error variant type from select element
      await commands.setMoonshineVariant(variant);
      const nextState = await commands.getRuntimeState();
      setRuntimeState(nextState);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "set_moonshine_variant_failed",
        message: error instanceof Error ? error.message : "Failed to update Moonshine variant.",
        recoverable: true,
      });
    }
  };

  const onSaveHotkeys = async () => {
    try {
      setShortcutCaptureTarget(null);
      await commands.setHotkeyBindings(
        pushShortcutDraft.trim(),
        toggleShortcutDraft.trim(),
      );
      const nextState = await commands.getRuntimeState();
      setRuntimeState(nextState);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "set_hotkeys_failed",
        message: error instanceof Error ? error.message : "Failed to update hotkeys.",
        recoverable: true,
      });
    }
  };

  const syncModelDownloads = async () => {
    const snapshot = await commands.listModelDownloads();
    setModelDownloads(snapshot);
  };

  const onStartModelDownload = async (modelKey: string) => {
    try {
      await commands.startModelDownload(modelKey);
      await syncModelDownloads();
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "model_download_start_failed",
        message: error instanceof Error ? error.message : "Failed to start model download.",
        recoverable: true,
      });
    }
  };

  const onCancelModelDownload = async (modelKey: string) => {
    try {
      await commands.cancelModelDownload(modelKey);
      await syncModelDownloads();
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "model_download_cancel_failed",
        message: error instanceof Error ? error.message : "Failed to cancel model download.",
        recoverable: true,
      });
    }
  };

  const onManualAction = async () => {
    try {
      if (runtimeState.phase === "recording") {
        const result = await commands.stopRecordingManual();
        setLatestTranscript(result.text);
        setOutputStatus(result.output_status);
        setClipboardRestored(result.clipboard_restored);
        setRuntimeUsed(result.runtime_used);
        setModelUsed(result.model_id);
        setDurationMs(result.duration_ms);
        setLatencyTargetMs(result.reliability_target_ms);
        setLatencyTargetMet(result.reliability_target_met);
      } else {
        await commands.startRecordingManual();
      }
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "manual_action_failed",
        message: error instanceof Error ? error.message : "Manual recording action failed.",
        recoverable: true,
      });
    }
  };

  const statusText = useMemo(() => {
    if (runtimeState.phase === "idle") return "Idle";
    if (runtimeState.phase === "recording") return "Recording";
    if (runtimeState.phase === "transcribing") return "Transcribing";
    return "Error";
  }, [runtimeState.phase]);

  const outputStatusText = useMemo(() => {
    if (outputStatus === "auto_pasted") return "Auto pasted";
    if (outputStatus === "paste_failed") return "Paste failed";
    if (outputStatus === "copied_only") return "Copied only";
    return "Idle";
  }, [outputStatus]);

  const showInterimPreview =
    runtimeState.phase === "recording" || runtimeState.phase === "transcribing";

  const transcriptDisplay = showInterimPreview ? interimTranscript : latestTranscript;

  const activeShortcutsSummary = `${runtimeState.push_to_talk_shortcut} / ${runtimeState.toggle_shortcut}`;
  const shortcutNotReady = shortcutInitError || !runtimeState.shortcut_ready;
  const showInputMonitoringReadiness =
    needsInputMonitoringPermission || runtimeState.input_monitoring_permission === "denied";
  const whisperModels = useMemo(
    () => modelDownloads.models.filter((model) => model.runtime === "whisper"),
    [modelDownloads.models],
  );
  const parakeetModels = useMemo(
    () => modelDownloads.models.filter((model) => model.runtime === "parakeet"),
    [modelDownloads.models],
  );
  const moonshineModels = useMemo(
    () => modelDownloads.models.filter((model) => model.runtime === "moonshine"),
    [modelDownloads.models],
  );

  const whisperModelByProfile = useMemo(() => {
    const modelsByProfile: Partial<Record<ModelProfile, ModelDownloadEntry>> = {};
    for (const model of whisperModels) {
      if (model.profile === "fast" || model.profile === "balanced" || model.profile === "accurate") {
        modelsByProfile[model.profile] = model;
      }
    }
    return modelsByProfile;
  }, [whisperModels]);

  const parakeetModelById = useMemo(() => {
    const modelsById: Record<string, ModelDownloadEntry> = {};
    for (const model of parakeetModels) {
      modelsById[model.model_id] = model;
    }
    return modelsById;
  }, [parakeetModels]);

  const moonshineModelByVariant = useMemo(() => {
    const modelsByVariant: Record<string, ModelDownloadEntry> = {};
    for (const model of moonshineModels) {
      modelsByVariant[model.profile] = model;
    }
    return modelsByVariant;
  }, [moonshineModels]);

  const isWhisperProfileReady = (profile: ModelProfile): boolean =>
    whisperModelByProfile[profile]?.status === "ready";

  const selectedWhisperModelReady =
    runtimeState.runtime_selection !== "whisper" || isWhisperProfileReady(runtimeState.model_profile);
  const selectedParakeetModelReady =
    runtimeState.runtime_selection !== "parakeet" ||
    parakeetModelById[runtimeState.parakeet_model_id]?.status === "ready";
  const selectedMoonshineModelReady =
    runtimeState.runtime_selection !== "moonshine" ||
    moonshineModelByVariant[runtimeState.moonshine_variant]?.status === "ready";
  const modelsReadyCount = modelDownloads.models.filter((model) => model.status === "ready").length;
  const modelsBadge =
    modelDownloads.active_model_key !== null
      ? "Downloading"
      : `${modelsReadyCount}/${Math.max(modelDownloads.models.length, 1)} Ready`;
  const hasModelDownloadFailure = modelDownloads.models.some((model) => model.status === "failed");

  const overviewTone: SettingsSectionTone =
    runtimeState.phase === "idle"
      ? "neutral"
      : runtimeState.phase === "recording"
        ? "danger"
        : "warning";

  const diagnosticsTone: SettingsSectionTone = runtimeError
    ? "danger"
    : lastReliabilityWarning
      ? "warning"
      : "neutral";

  const diagnosticsBadge = runtimeError
    ? "Error"
    : lastReliabilityWarning
      ? "Warning"
      : "Healthy";

  // Drive screens from one extendable section registry.

  const settingsSections: SettingsSection[] = [
    {
      id: "overview",
      title: "Overview",
      description: "Live status and quick recording controls.",
      badge: statusText,
      tone: overviewTone,
      content: (
        <div className="section-stack">
          <section className="section-block status-strip">
            <div>
              <p className="label">Status</p>
              <p className={`status ${runtimeState.phase}`}>{statusText}</p>
            </div>

            <div>
              <p className="label">Global Shortcuts</p>
              <p className="value">{activeShortcutsSummary}</p>
            </div>

            <div>
              <p className="label">Runtime</p>
              <p className="value runtime">{runtimeState.runtime_selection}</p>
            </div>

            <button
              className="btn primary"
              onClick={onManualAction}
              disabled={runtimeState.phase === "transcribing"}
            >
              {runtimeState.phase === "recording" ? "Stop + Transcribe" : "Start Recording"}
            </button>
          </section>

          {showInterimPreview && (
            <section className="section-block preview-panel">
              <div className="transcript-head">
                <p className="label">Live Preview</p>
                <p className="caption">
                  {runtimeState.phase === "recording"
                    ? "Recording in progress"
                    : "Finalizing transcript"}
                </p>
              </div>
              <p className="live-preview-text">
                {interimTranscript.trim().length > 0
                  ? interimTranscript
                  : runtimeState.runtime_selection === "parakeet"
                    ? "Recording in progress..."
                    : "Listening for interim transcription..."}
              </p>
            </section>
          )}

          {showInterimPreview && (
            <section className="section-block preview-panel">
              <div className="transcript-head">
                <p className="label">Word Stream (Experimental)</p>
                <p className="caption">Silence-segmented live decoding</p>
              </div>
              <p className="live-preview-text">
                {liveWordTranscript.trim().length > 0
                  ? liveWordTranscript
                  : "Listening for word boundaries..."}
              </p>
            </section>
          )}

          {shortcutNotReady && (
            <section className="section-block error-panel">
              <p className="label">Shortcut Not Ready</p>
              <p>
                Steno could not initialize one or more global shortcuts. Check Input Monitoring
                and Accessibility permissions, then apply different shortcuts if needed.
              </p>
              {(shortcutInitError ?? runtimeError) && (
                <p className="caption">{(shortcutInitError ?? runtimeError)?.message}</p>
              )}
            </section>
          )}

          {showInputMonitoringReadiness && (
            <section className="section-block warning-panel">
              <p className="label">Input Monitoring Needed</p>
              <p>Global shortcuts unavailable until Input Monitoring is granted.</p>
              <button onClick={grantInputMonitoringPermission} className="btn secondary">
                Grant Input Monitoring Access
              </button>
            </section>
          )}
        </div>
      ),
    },
    {
      id: "input",
      title: "Input Settings",
      description: "Dual global hotkey bindings.",
      badge: "Dual",
      tone: "neutral",
      content: (
        <div className="section-stack">
          <section className="section-block settings-card">
            <label className="field">
              <span className="label">Push-to-talk Shortcut</span>
              <div className="shortcut-field-row">
                <input
                  type="text"
                  value={pushShortcutDraft}
                  onChange={(event) => setPushShortcutDraft(event.target.value)}
                  placeholder="Fn"
                  className="input"
                />
                <button
                  className="btn secondary shortcut-capture-btn"
                  onClick={() => onShortcutCaptureToggle("push")}
                  disabled={runtimeState.phase === "transcribing" || shortcutCaptureTarget !== null}
                >
                  {shortcutCaptureTarget === "push" ? "Press keys..." : "Record Shortcut"}
                </button>
              </div>
            </label>

            <label className="field">
              <span className="label">Toggle Shortcut</span>
              <div className="shortcut-field-row">
                <input
                  type="text"
                  value={toggleShortcutDraft}
                  onChange={(event) => setToggleShortcutDraft(event.target.value)}
                  placeholder="Shift+Fn"
                  className="input"
                />
                <button
                  className="btn secondary shortcut-capture-btn"
                  onClick={() => onShortcutCaptureToggle("toggle")}
                  disabled={runtimeState.phase === "transcribing" || shortcutCaptureTarget !== null}
                >
                  {shortcutCaptureTarget === "toggle" ? "Press keys..." : "Record Shortcut"}
                </button>
              </div>
            </label>

            <button
              className="btn secondary"
              onClick={onSaveHotkeys}
              disabled={runtimeState.phase === "transcribing" || shortcutCaptureTarget !== null}
            >
              Apply Hotkeys
            </button>

            <p className="caption">
              Both shortcuts stay active together. Click Record Shortcut, press your keys, then
              Apply Hotkeys. Press <code>Esc</code> to cancel capture.
            </p>
            <p className="caption">
              Capture listens globally while recording, not just this window.
            </p>
            <p className="caption">
              Use combos like <code>Fn</code>, <code>Ctrl+Fn</code>, or <code>Cmd+Shift+Space</code>.
            </p>
          </section>
        </div>
      ),
    },
    {
      id: "transcription",
      title: "Transcription Settings",
      description: "Runtime, model selection, and clipboard output policy.",
      badge: runtimeState.runtime_selection,
      tone: "neutral",
      content: (
        <div className="section-stack">
          <section className="section-block settings-card">
            <label className="field">
              <span className="label">Runtime</span>
              <select
                value={runtimeState.runtime_selection}
                onChange={(event) =>
                  onRuntimeSelectionChange(event.target.value as TranscriptionRuntime)
                }
                disabled={runtimeState.phase === "transcribing"}
                className="select"
              >
                <option value="whisper">Whisper</option>
                <option value="parakeet">Parakeet</option>
                <option value="moonshine">Moonshine</option>
              </select>
            </label>

            {runtimeState.runtime_selection === "whisper" && (
              <>
                <label className="field">
                  <span className="label">Model Profile</span>
                  <select
                    value={runtimeState.model_profile}
                    onChange={(event) => onModelProfileChange(event.target.value as ModelProfile)}
                    disabled={runtimeState.phase === "transcribing"}
                    className="select"
                  >
                    {(["fast", "balanced", "accurate"] as ModelProfile[]).map((profile) => {
                      const modelEntry = whisperModelByProfile[profile];
                      const isReady = modelEntry?.status === "ready";
                      const shouldDisable = !isReady && runtimeState.model_profile !== profile;
                      const label = profile.charAt(0).toUpperCase() + profile.slice(1);
                      const optionLabel = isReady ? label : `${label} (download required)`;

                      return (
                        <option key={profile} value={profile} disabled={shouldDisable}>
                          {optionLabel}
                        </option>
                      );
                    })}
                  </select>
                </label>

                {!selectedWhisperModelReady && (
                  <section className="warning-panel model-warning-callout">
                    <p className="caption warning">
                      Selected Whisper profile is not downloaded yet.
                    </p>
                    <button
                      className="btn secondary"
                      type="button"
                      onClick={() => setActiveSectionId("models")}
                    >
                      Open Models Section
                    </button>
                  </section>
                )}
              </>
            )}

            {runtimeState.runtime_selection === "parakeet" && (
              <>
                <label className="field">
                  <span className="label">Parakeet Model</span>
                  <select
                    value={runtimeState.parakeet_model_id}
                    onChange={(event) => onParakeetModelChange(event.target.value)}
                    disabled={
                      runtimeState.phase === "transcribing" ||
                      parakeetModels.length === 0
                    }
                    className="select"
                  >
                    {parakeetModels.length === 0 ? (
                      <option value={runtimeState.parakeet_model_id} disabled>
                        Loading model catalog...
                      </option>
                    ) : (
                      parakeetModels.map((model) => {
                        const label =
                          model.status === "ready"
                            ? `${model.profile.toUpperCase()} (${model.model_id})`
                            : `${model.profile.toUpperCase()} (${model.model_id}) - download required`;
                        return (
                          <option key={model.key} value={model.model_id}>
                            {label}
                          </option>
                        );
                      })
                    )}
                  </select>
                </label>

                {!selectedParakeetModelReady && (
                  <section className="warning-panel model-warning-callout">
                    <p className="caption warning">
                      Selected Parakeet model is not downloaded yet.
                    </p>
                    <button
                      className="btn secondary"
                      type="button"
                      onClick={() => setActiveSectionId("models")}
                    >
                      Open Models Section
                    </button>
                  </section>
                )}
              </>
            )}

            {runtimeState.runtime_selection === "moonshine" && (
              <>
                <label className="field">
                  <span className="label">Moonshine Variant</span>
                  <select
                    value={runtimeState.moonshine_variant}
                    onChange={(event) => onMoonshineVariantChange(event.target.value)}
                    disabled={runtimeState.phase === "transcribing" || moonshineModels.length === 0}
                    className="select"
                  >
                    {moonshineModels.length === 0 ? (
                      <option value={runtimeState.moonshine_variant} disabled>
                        Loading model catalog...
                      </option>
                    ) : (
                      moonshineModels.map((model) => {
                        const label =
                          model.status === "ready"
                            ? `${model.profile.toUpperCase()} (${model.model_id})`
                            : `${model.profile.toUpperCase()} (${model.model_id}) - download required`;
                        return (
                          <option key={model.key} value={model.profile}>
                            {label}
                          </option>
                        );
                      })
                    )}
                  </select>
                </label>

                {!selectedMoonshineModelReady && (
                  <section className="warning-panel model-warning-callout">
                    <p className="caption warning">
                      Selected Moonshine variant is not downloaded yet.
                    </p>
                    <button
                      className="btn secondary"
                      type="button"
                      onClick={() => setActiveSectionId("models")}
                    >
                      Open Models Section
                    </button>
                  </section>
                )}
              </>
            )}

            <label className="field">
              <span className="label">Output Clipboard Policy</span>
              <select
                value={runtimeState.clipboard_policy}
                onChange={(event) =>
                  onClipboardPolicyChange(event.target.value as ClipboardPolicy)
                }
                disabled={runtimeState.phase === "transcribing"}
                className="select"
              >
                <option value="restore_previous">Restore previous clipboard</option>
                <option value="keep_transcript">Keep transcript in clipboard</option>
              </select>
            </label>

            {runtimeState.runtime_selection === "whisper" && (
              <p className="caption">
                Whisper profiles require downloaded artifacts from the Models section.
              </p>
            )}
            {runtimeState.runtime_selection === "parakeet" && (
              <p className="caption">
                Parakeet models are downloaded and managed in the Models section.
              </p>
            )}
            {runtimeState.runtime_selection === "moonshine" && (
              <p className="caption">
                Moonshine models are downloaded and managed in the Models section.
              </p>
            )}
          </section>
        </div>
      ),
    },
    {
      id: "models",
      title: "Models",
      description: "Download and manage Whisper and Parakeet model artifacts.",
      badge: modelsBadge,
      tone: hasModelDownloadFailure ? "warning" : "neutral",
      content: (
        <div className="section-stack">
          <section className="section-block">
            <p className="caption">
              Queue: {modelDownloads.queue.length} pending
              {modelDownloads.active_model_key
                ? ` • Active: ${modelDownloads.active_model_key}`
                : " • Active: none"}
            </p>
          </section>

          <section className="section-block models-grid">
            {modelDownloads.models.length === 0 && (
              <p className="caption">Loading model catalog...</p>
            )}

            {modelDownloads.models.map((model) => {
              const progressMax = Math.max(model.total_bytes, 1);
              const progressValue = Math.min(model.downloaded_bytes, progressMax);
              const progressPercent = model.total_bytes > 0
                ? Math.min(100, Math.round((model.downloaded_bytes / model.total_bytes) * 100))
                : 0;
              const queuePosition = modelDownloads.queue.indexOf(model.key);
              const queueOrdinal = queuePosition >= 0 ? queuePosition + 1 : null;
              const isActive = modelDownloads.active_model_key === model.key;

              return (
                <article key={model.key} className="model-card">
                  <div className="model-card-header">
                    <div>
                      <p className="label">
                        {model.runtime === "whisper"
                          ? `${model.profile} profile`
                          : `${model.profile.toUpperCase()} version`}
                      </p>
                      <p className="caption">{model.runtime}</p>
                      <p className="model-id">{model.model_id}</p>
                    </div>
                    <span className={`model-status status-${model.status}`}>
                      {formatModelStatus(model.status)}
                    </span>
                  </div>

                  <p className="caption model-files">
                    Files: {model.required_files.join(", ")}
                  </p>

                  <progress className="model-progress" max={progressMax} value={progressValue} />
                  <p className="caption">
                    {modelProgressLabel(model)}
                    {model.total_bytes > 0 ? ` • ${progressPercent}%` : ""}
                  </p>

                  {(model.status === "queued" || isActive) && (
                    <p className="caption">
                      {isActive
                        ? "Download in progress."
                        : queueOrdinal === null
                          ? "Queued."
                          : `Queued (#${queueOrdinal}).`}
                    </p>
                  )}

                  {model.last_error && model.status === "failed" && (
                    <p className="caption warning">{model.last_error}</p>
                  )}

                  <div className="model-card-actions">
                    {model.status === "ready" && (
                      <button className="btn secondary" disabled type="button">
                        Ready
                      </button>
                    )}
                    {(model.status === "queued" || model.status === "downloading") && (
                      <button
                        className="btn secondary"
                        type="button"
                        onClick={() => onCancelModelDownload(model.key)}
                      >
                        Cancel
                      </button>
                    )}
                    {(model.status === "failed" || model.status === "canceled") && (
                      <button
                        className="btn primary"
                        type="button"
                        onClick={() => onStartModelDownload(model.key)}
                      >
                        Retry
                      </button>
                    )}
                    {model.status === "not_downloaded" && (
                      <button
                        className="btn primary"
                        type="button"
                        onClick={() => onStartModelDownload(model.key)}
                      >
                        Download
                      </button>
                    )}
                  </div>
                </article>
              );
            })}
          </section>
        </div>
      ),
    },
    {
      id: "transcript",
      title: "Transcript",
      description: "Latest output and post-processing metadata.",
      badge: outputStatusText,
      tone: outputStatus === "paste_failed" ? "danger" : "neutral",
      content: (
        <div className="section-stack">
          <section className="section-block">
            <div className="transcript-head">
              <p className="label">{showInterimPreview ? "Live Preview" : "Latest Transcript"}</p>
              <p className="caption">Output: {outputStatusText}</p>
            </div>

            <textarea
              value={transcriptDisplay}
              readOnly
              placeholder={
                showInterimPreview
                  ? "Listening for interim transcription..."
                  : "Your transcript will appear here..."
              }
              className="transcript"
            />

            <div className="meta-grid">
              <p className="caption">Runtime used: {runtimeUsed ?? "Not run yet"}</p>
              <p className="caption">Model: {modelUsed || "Not run yet"}</p>
              <p className="caption">
                Duration: {durationMs === null ? "Not run yet" : `${durationMs}ms`}
              </p>
              <p className="caption">
                Latency target: {latencyTargetMs === null
                  ? "Not run yet"
                  : latencyTargetMet
                    ? `Met (${latencyTargetMs}ms target)`
                    : `Missed (${latencyTargetMs}ms target)`}
              </p>
              {lastReliabilityWarning && (
                <p className="caption warning">Last reliability warning: {lastReliabilityWarning}</p>
              )}
              {runtimeState.clipboard_policy === "restore_previous" && (
                <p className="caption">
                  Clipboard restore: {clipboardRestored === null
                    ? "Not attempted"
                    : clipboardRestored
                      ? "Restored"
                      : "Failed"}
                </p>
              )}
            </div>
          </section>
        </div>
      ),
    },
    {
      id: "diagnostics",
      title: "Diagnostics",
      description: "Runtime errors, warnings, and permission readiness.",
      badge: diagnosticsBadge,
      tone: diagnosticsTone,
      content: (
        <div className="section-stack">
          <section className={`section-block ${runtimeError ? "error-panel" : ""}`.trim()}>
            <p className="label">Runtime Error</p>
            {runtimeError ? (
              <>
                <p>{runtimeError.message}</p>
                <p className="caption">Code: {runtimeError.code}</p>
              </>
            ) : (
              <p className="caption">No active runtime errors.</p>
            )}
          </section>

          <section
            className={`section-block ${lastReliabilityWarning ? "warning-panel" : ""}`.trim()}
          >
            <p className="label">Reliability Signal</p>
            {lastReliabilityWarning ? (
              <p>{lastReliabilityWarning}</p>
            ) : (
              <p className="caption">No reliability warnings captured.</p>
            )}
          </section>

          <section className="section-block">
            <p className="label">Permissions</p>
            <div className="meta-grid">
              <p className="caption">
                Accessibility: {needsAccessibilityPermission ? "Missing" : "Granted"}
              </p>
              <p className="caption">Microphone: {needsMicPermission ? "Missing" : "Granted"}</p>
              <p className="caption">
                Input Monitoring: {showInputMonitoringReadiness ? "Missing" : "Granted"}
              </p>
              <p className="caption">
                Shortcut readiness: {runtimeState.shortcut_ready ? "Ready" : "Not ready"}
              </p>
            </div>
          </section>
        </div>
      ),
    },
  ];

  const activeSection =
    settingsSections.find((section) => section.id === activeSectionId) ?? settingsSections[0];

  if (loading) {
    return (
      <main className="screen center">
        <p>Initializing Steno runtime...</p>
      </main>
    );
  }

  if (!isMac) {
    return (
      <main className="screen center">
        <h1>Steno</h1>
        <p>Steno is macOS-only.</p>
      </main>
    );
  }

  if (needsMicPermission) {
    return (
      <main className="screen center">
        <h1>Microphone Permission Needed</h1>
        <p>
          Steno needs microphone access to record audio. Grant permission, then continue.
        </p>
        <button onClick={grantMicPermission} className="btn primary">
          Grant Microphone Access
        </button>
        {runtimeError && <pre className="error-block">{runtimeError.message}</pre>}
      </main>
    );
  }

  if (needsAccessibilityPermission) {
    return (
      <main className="screen center">
        <h1>Accessibility Permission Needed</h1>
        <p>
          Steno needs Accessibility access to register global shortcuts. Grant permission,
          then continue.
        </p>
        <button onClick={grantAccessibilityPermission} className="btn primary">
          Grant Accessibility Access
        </button>
        {runtimeError && <pre className="error-block">{runtimeError.message}</pre>}
      </main>
    );
  }

  return (
    <main className="screen">
      <header className="header">
        <h1>Steno</h1>
        <p className="subtitle">Always-on local dictation for macOS</p>
      </header>

      <div className={`workspace-layout ${isSidebarCollapsed ? "sidebar-collapsed" : ""}`.trim()}>
        <SettingsNavigator
          sections={settingsSections}
          activeSectionId={activeSection.id}
          isCollapsed={isSidebarCollapsed}
          onSelect={setActiveSectionId}
          onToggleCollapse={() => setIsSidebarCollapsed((prev) => !prev)}
        />

        <SettingsContentFrame section={activeSection} />
      </div>
    </main>
  );
}

type SettingsNavigatorProps = {
  sections: SettingsSection[];
  activeSectionId: SettingsSectionId;
  isCollapsed: boolean;
  onSelect: (id: SettingsSectionId) => void;
  onToggleCollapse: () => void;
};

function SettingsNavigator({
  sections,
  activeSectionId,
  isCollapsed,
  onSelect,
  onToggleCollapse,
}: SettingsNavigatorProps) {
  const toggleLabel = isCollapsed ? "Expand Sidebar" : "Collapse Sidebar";

  return (
    <aside className={`panel nav-panel ${isCollapsed ? "is-collapsed" : ""}`.trim()}>
      <div className="nav-header">
        <p className="label nav-title">Settings Navigator</p>
        <button
          type="button"
          className={`sidebar-toggle ${isCollapsed ? "is-collapsed" : ""}`.trim()}
          onClick={onToggleCollapse}
          aria-label={toggleLabel}
          title={toggleLabel}
        >
          <span className="sidebar-toggle-glyph" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
              <rect x="3.5" y="4.5" width="17" height="15" rx="3" />
              <path d="M8.5 4.5v15" />
              <path className="sidebar-toggle-arrow" d="M14.5 9.2 11.2 12l3.3 2.8" />
            </svg>
          </span>
          <span className="sidebar-toggle-label" aria-hidden="true">
            {isCollapsed ? "Open" : "Hide"}
          </span>
        </button>
      </div>

      <nav className="nav-list" aria-label="Settings sections">
        {sections.map((section) => {
          const toneClass = `tone-${section.tone ?? "neutral"}`;
          const isActive = section.id === activeSectionId;
          const sectionIcon = getSectionIcon(section.id);

          return (
            <button
              key={section.id}
              className={`nav-item ${toneClass} ${isActive ? "is-active" : ""}`.trim()}
              onClick={() => onSelect(section.id)}
              type="button"
              title={section.title}
              aria-label={section.title}
              aria-current={isActive ? "page" : undefined}
            >
              <span className="nav-item-icon" aria-hidden="true">
                {sectionIcon}
              </span>
              <span className="nav-item-main">{section.title}</span>
              <span className="nav-item-helper">{section.description}</span>
              {section.badge && <span className={`nav-badge ${toneClass}`}>{section.badge}</span>}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}

type SettingsContentFrameProps = {
  section: SettingsSection;
};

function SettingsContentFrame({ section }: SettingsContentFrameProps) {
  return (
    <section className="panel content-panel">
      <div className="content-header">
        <p className="label">Section</p>
        <h2>{section.title}</h2>
        <p className="content-description">{section.description}</p>
      </div>

      {section.content}
    </section>
  );
}

function isReliabilityWarningCode(code: string): boolean {
  return (
    code === "latency_target_missed" ||
    code === "recording_start_timeout" ||
    code === "recording_stop_timeout" ||
    code === "transcription_timeout" ||
    code === "output_action_timeout" ||
    code === "focus_anchor_failed" ||
    code === "focus_recheck_failed" ||
    code === "focus_drift_detected" ||
    code === "auto_paste_skipped_for_safety" ||
    code === "interim_auto_disabled"
  );
}

function getSectionIcon(sectionId: SettingsSectionId): ReactNode {
  if (sectionId === "overview") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
        <path d="M4 12a8 8 0 1 1 16 0" />
        <path d="M8 12a4 4 0 0 1 8 0" />
        <circle cx="12" cy="12" r="1.5" />
      </svg>
    );
  }

  if (sectionId === "input") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
        <rect x="3.5" y="6.5" width="17" height="11" rx="2.5" />
        <path d="M7 10h1M10 10h1M13 10h1M16 10h1" />
        <path d="M7 13h10" />
      </svg>
    );
  }

  if (sectionId === "transcription") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
        <path d="M7 10.5a5 5 0 0 1 10 0v2a5 5 0 0 1-10 0v-2Z" />
        <path d="M5 12.5a7 7 0 0 0 14 0" />
        <path d="M12 19v2.5M9.5 21.5h5" />
      </svg>
    );
  }

  if (sectionId === "models") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
        <path d="M12 3.5 5.5 7 12 10.5 18.5 7 12 3.5Z" />
        <path d="M5.5 7v4L12 14.5V10.5L5.5 7Z" />
        <path d="M18.5 7v4L12 14.5V10.5L18.5 7Z" />
        <path d="M5.5 13 12 16.5 18.5 13" />
      </svg>
    );
  }

  if (sectionId === "transcript") {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
        <path d="M7 4.5h8.5L19 8v11a1.5 1.5 0 0 1-1.5 1.5H7A1.5 1.5 0 0 1 5.5 19V6A1.5 1.5 0 0 1 7 4.5Z" />
        <path d="M15.5 4.5V8H19" />
        <path d="M8.5 11.5h7M8.5 14.5h7" />
      </svg>
    );
  }

  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor">
      <path d="M12 3.5 4.5 7v5c0 4.3 3.2 7.6 7.5 8.5 4.3-.9 7.5-4.2 7.5-8.5V7L12 3.5Z" />
      <path d="m9.5 12 1.8 1.8L14.8 10" />
    </svg>
  );
}

export default App;
