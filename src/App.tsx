import { useEffect, useMemo, useState } from "react";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { commands, events } from "./tauri";
import type {
  RecordMode,
  RuntimeError,
  RuntimeInitResult,
  RuntimeState,
  TranscriptionResult,
} from "./types";

const initialState: RuntimeState = {
  phase: "idle",
  mode: "push_to_talk",
  shortcut_ready: false,
  mic_permission: "unknown",
};

const STARTUP_TIMEOUT_MS = 8000;
const LISTENER_TIMEOUT_MS = 3000;
const STARTUP_WATCHDOG_MS = 15000;

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

function App() {
  const [isMac, setIsMac] = useState(false);
  const [runtimeState, setRuntimeState] = useState<RuntimeState>(initialState);
  const [latestTranscript, setLatestTranscript] = useState("");
  const [clipboardStatus, setClipboardStatus] = useState<"idle" | "copied" | "failed">(
    "idle",
  );
  const [runtimeError, setRuntimeError] = useState<RuntimeError | null>(null);
  const [loading, setLoading] = useState(true);
  const [needsAccessibilityPermission, setNeedsAccessibilityPermission] = useState(false);
  const [needsMicPermission, setNeedsMicPermission] = useState(false);
  const [shortcutInitError, setShortcutInitError] = useState<RuntimeError | null>(null);

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

  useEffect(() => {
    let disposed = false;
    let startupSettled = false;
    let startupWatchdog: ReturnType<typeof setTimeout> | undefined;
    let unlistenState: (() => void) | undefined;
    let unlistenTranscription: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;

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
            setRuntimeState(state);
          }),
          LISTENER_TIMEOUT_MS,
          "State listener registration",
        );

        const transcriptionUnlisten = await withTimeout(
          events.onTranscriptionComplete((result: TranscriptionResult) => {
            setLatestTranscript(result.text);
            setClipboardStatus(result.copied_to_clipboard ? "copied" : "failed");
            setRuntimeError(null);
          }),
          LISTENER_TIMEOUT_MS,
          "Transcription listener registration",
        );

        const errorUnlisten = await withTimeout(
          events.onError((error) => {
            setRuntimeError(error);
          }),
          LISTENER_TIMEOUT_MS,
          "Error listener registration",
        );

        if (disposed) {
          stateUnlisten();
          transcriptionUnlisten();
          errorUnlisten();
          return;
        }

        unlistenState = stateUnlisten;
        unlistenTranscription = transcriptionUnlisten;
        unlistenError = errorUnlisten;
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
            message: "Steno v1 currently supports macOS only.",
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
      unlistenError?.();
    };
  }, []);

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

  const onModeChange = async (mode: RecordMode) => {
    try {
      await commands.setRecordMode(mode);
      const nextState = await commands.getRuntimeState();
      setRuntimeState(nextState);
      setRuntimeError(null);
    } catch (error) {
      setRuntimeError({
        code: "set_mode_failed",
        message: error instanceof Error ? error.message : "Failed to update recording mode.",
        recoverable: true,
      });
    }
  };

  const onManualAction = async () => {
    try {
      if (runtimeState.phase === "recording") {
        const result = await commands.stopRecordingManual();
        setLatestTranscript(result.text);
        setClipboardStatus(result.copied_to_clipboard ? "copied" : "failed");
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
        <p>Steno v1 is macOS-only.</p>
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
          Steno needs Accessibility access to register and listen for the global Fn shortcut.
          Grant permission, then continue.
        </p>
        <button onClick={grantAccessibilityPermission} className="btn primary">
          Grant Accessibility Access
        </button>
        {runtimeError && <pre className="error-block">{runtimeError.message}</pre>}
      </main>
    );
  }

  if (shortcutInitError || !runtimeState.shortcut_ready) {
    const displayedError = shortcutInitError ?? runtimeError;
    return (
      <main className="screen center">
        <h1>Fn Shortcut Not Ready</h1>
        <p>
          Steno could not initialize the global Fn shortcut. Enable the required macOS keyboard
          permissions (Input Monitoring / Accessibility), then restart the app.
        </p>
        {displayedError && <pre className="error-block">{displayedError.message}</pre>}
      </main>
    );
  }

  return (
    <main className="screen">
      <header className="header">
        <h1>Steno</h1>
        <p className="subtitle">Fn-triggered local transcription with Rust Whisper</p>
      </header>

      <section className="panel row">
        <div>
          <p className="label">Status</p>
          <p className={`status ${runtimeState.phase}`}>{statusText}</p>
        </div>

        <div>
          <p className="label">Mode</p>
          <select
            value={runtimeState.mode}
            onChange={(event) => onModeChange(event.target.value as RecordMode)}
            disabled={runtimeState.phase === "transcribing"}
            className="select"
          >
            <option value="push_to_talk">Push-to-talk (Fn hold)</option>
            <option value="toggle">Toggle (Fn tap)</option>
          </select>
        </div>

        <button
          className="btn primary"
          onClick={onManualAction}
          disabled={runtimeState.phase === "transcribing"}
        >
          {runtimeState.phase === "recording" ? "Stop + Transcribe" : "Start Recording"}
        </button>
      </section>

      <section className="panel">
        <p className="label">Latest Transcript</p>
        <textarea
          value={latestTranscript}
          readOnly
          placeholder="Your transcript will appear here..."
          className="transcript"
        />
        <p className="caption">
          Clipboard: {clipboardStatus === "copied" ? "Copied" : clipboardStatus === "failed" ? "Failed" : "Idle"}
        </p>
      </section>

      {runtimeError && (
        <section className="panel error-panel">
          <p className="label">Error</p>
          <p>{runtimeError.message}</p>
          <p className="caption">Code: {runtimeError.code}</p>
        </section>
      )}

      <section className="panel">
        <p className="caption">
          Shortcut: <code>Fn</code> only. No fallback key in v1.
        </p>
      </section>
    </main>
  );
}

export default App;
