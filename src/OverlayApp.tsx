import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands, events } from "./tauri";
import type { InterimTranscriptionFrame, Phase } from "./types";

type OverlayPhase = "hidden" | "recording" | "transcribing" | "done";
const DONE_DISPLAY_MS = 500;

function OverlayApp() {
  const [overlayPhase, setOverlayPhase] = useState<OverlayPhase>("hidden");
  const [interimPreview, setInterimPreview] = useState("");
  const previousPhaseRef = useRef<Phase>("idle");
  const activePhaseRef = useRef<Phase>("idle");
  const doneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlistenState: (() => void) | undefined;
    let unlistenInterim: (() => void) | undefined;
    const currentWindow = getCurrentWindow();

    const clearDoneTimer = () => {
      if (doneTimerRef.current) {
        clearTimeout(doneTimerRef.current);
        doneTimerRef.current = null;
      }
    };

    const hideOverlay = async () => {
      try {
        await currentWindow.hide();
      } catch {
        // Ignore overlay hide failures.

      }
    };

    const showOverlay = async () => {
      try {
        await currentWindow.show();
      } catch {
        // Ignore overlay show failures.

      }
    };

    const applyPhase = (phase: Phase) => {
      const previousPhase = previousPhaseRef.current;
      previousPhaseRef.current = phase;
      activePhaseRef.current = phase;

      clearDoneTimer();

      if (phase === "recording") {
        setOverlayPhase("recording");
        void showOverlay();
        return;
      }

      if (phase === "transcribing") {
        setOverlayPhase("transcribing");
        void showOverlay();
        return;
      }

      if (phase === "idle" && previousPhase === "transcribing") {
        setOverlayPhase("done");
        void showOverlay();
        doneTimerRef.current = setTimeout(() => {
          if (disposed) {
            return;
          }
          setInterimPreview("");
          setOverlayPhase("hidden");
          void hideOverlay();
        }, DONE_DISPLAY_MS);
        return;
      }

      if (phase === "idle" || phase === "error") {
        setInterimPreview("");
      }

      setOverlayPhase("hidden");
      void hideOverlay();
    };

    // Mark overlay document for transparent styling.

    document.documentElement.classList.add("overlay-body");
    document.body.classList.add("overlay-body");

    const init = async () => {
      try {
        const state = await commands.getRuntimeState();
        if (!disposed) {
          applyPhase(state.phase);
        }
      } catch {
        // Ignore init state fetch failures.

      }

      try {
        const stateUnlisten = await events.onStateChanged((state) => {
          applyPhase(state.phase);
        });
        const interimUnlisten = await events.onInterimTranscription(
          (frame: InterimTranscriptionFrame) => {
            if (activePhaseRef.current !== "recording") {
              return;
            }
            setInterimPreview(frame.text);
          },
        );
        if (disposed) {
          stateUnlisten();
          interimUnlisten();
          return;
        }
        unlistenState = stateUnlisten;
        unlistenInterim = interimUnlisten;
      } catch {
        // Ignore listener setup failures.

      }
    };

    void init();

    return () => {
      disposed = true;
      clearDoneTimer();
      unlistenState?.();
      unlistenInterim?.();
      document.documentElement.classList.remove("overlay-body");
      document.body.classList.remove("overlay-body");
    };
  }, []);

  if (overlayPhase === "hidden") {
    return <main className="overlay-root" />;
  }

  const overlayLabel =
    overlayPhase === "recording"
      ? "Recording"
      : overlayPhase === "transcribing"
        ? "Transcribing"
        : "Done";

  const overlayStateClass =
    overlayPhase === "recording"
      ? "is-recording"
      : overlayPhase === "transcribing"
        ? "is-transcribing"
        : "is-done";

  const overlayPreview = interimPreview;

  const showPreviewLine =
    (overlayPhase === "recording" || overlayPhase === "transcribing") &&
    overlayPreview.trim().length > 0;

  return (
    <main className="overlay-root">
      <section className={`overlay-indicator ${overlayStateClass}`} aria-live="polite">
        <span className={`overlay-dot ${overlayStateClass}`} />
        <span className="overlay-text-stack">
          <span className="overlay-text">{overlayLabel}</span>
          {showPreviewLine && <span className="overlay-preview">{overlayPreview}</span>}
        </span>
      </section>
    </main>
  );
}

export default OverlayApp;
