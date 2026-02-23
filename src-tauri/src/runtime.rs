use crate::audio_capture::{write_wav_file, AudioCapture, CapturedAudio};
use crate::post_process::{NoopPostProcessor, PostProcessor};
use crate::shortcut::FnShortcutManager;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use transcriber_core::transcribe_file;

const MIN_AUDIO_SECONDS: f32 = 0.20;
const DEFAULT_MODEL_ID: &str = "openai/whisper-tiny";
const TOGGLE_DEBOUNCE_MS: u64 = 180;
const HOTKEY_INIT_TIMEOUT_MS: u64 = 5000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordMode {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MicPermission {
    Unknown,
    Granted,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeState {
    pub phase: Phase,
    pub mode: RecordMode,
    pub shortcut_ready: bool,
    pub mic_permission: MicPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInitResult {
    pub state: RuntimeState,
    pub shortcut_error: Option<RuntimeError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub model_id: String,
    pub duration_ms: u64,
    pub copied_to_clipboard: bool,
}

enum AudioCommand {
    Start {
        response: mpsc::Sender<Result<(), String>>,
    },
    Stop {
        response: mpsc::Sender<Result<CapturedAudio, String>>,
    },
    IsRecording {
        response: mpsc::Sender<bool>,
    },
    Shutdown,
}

struct AudioWorker {
    tx: mpsc::Sender<AudioCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

impl AudioWorker {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<AudioCommand>();

        let handle = thread::spawn(move || {
            let mut capture = AudioCapture::new();

            while let Ok(command) = rx.recv() {
                match command {
                    AudioCommand::Start { response } => {
                        let result = capture.start().map_err(|e| e.to_string());
                        let _ = response.send(result);
                    }
                    AudioCommand::Stop { response } => {
                        let result = capture.stop().map_err(|e| e.to_string());
                        let _ = response.send(result);
                    }
                    AudioCommand::IsRecording { response } => {
                        let _ = response.send(capture.is_recording());
                    }
                    AudioCommand::Shutdown => break,
                }
            }
        });

        Self {
            tx,
            handle: Some(handle),
        }
    }

    fn start(&self) -> Result<(), String> {
        let (response_tx, response_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::Start {
                response: response_tx,
            })
            .map_err(|e| format!("Failed to send start command to audio worker: {}", e))?;

        response_rx
            .recv()
            .map_err(|e| format!("Failed to receive start response from audio worker: {}", e))?
    }

    fn stop(&self) -> Result<CapturedAudio, String> {
        let (response_tx, response_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::Stop {
                response: response_tx,
            })
            .map_err(|e| format!("Failed to send stop command to audio worker: {}", e))?;

        response_rx
            .recv()
            .map_err(|e| format!("Failed to receive stop response from audio worker: {}", e))?
    }

    fn is_recording(&self) -> bool {
        let (response_tx, response_rx) = mpsc::channel();
        if self
            .tx
            .send(AudioCommand::IsRecording {
                response: response_tx,
            })
            .is_err()
        {
            return false;
        }

        response_rx.recv().unwrap_or(false)
    }
}

impl Drop for AudioWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct RuntimeInner {
    state: RuntimeState,
    model_id: String,
    audio: AudioWorker,
    shortcut_manager: Option<FnShortcutManager>,
    last_toggle_press: Option<Instant>,
}

#[derive(Clone)]
pub struct RuntimeController {
    inner: Arc<Mutex<RuntimeInner>>,
}

impl RuntimeController {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeInner {
                state: RuntimeState {
                    phase: Phase::Idle,
                    mode: RecordMode::PushToTalk,
                    shortcut_ready: false,
                    mic_permission: MicPermission::Unknown,
                },
                model_id: DEFAULT_MODEL_ID.to_string(),
                audio: AudioWorker::new(),
                shortcut_manager: None,
                last_toggle_press: None,
            })),
        }
    }

    pub fn initialize(&self, app: &AppHandle) -> RuntimeInitResult {
        let mut shortcut_error = None;

        let should_init_shortcut = {
            let inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.shortcut_manager.is_none()
        };

        if should_init_shortcut {
            match initialize_shortcut_with_timeout(app.clone(), HOTKEY_INIT_TIMEOUT_MS) {
                Ok(manager) => {
                    let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
                    inner.shortcut_manager = Some(manager);
                    inner.state.shortcut_ready = true;
                    log::info!("event=shortcut_initialized key=fn");
                }
                Err(message) => {
                    let err = runtime_error(
                        "shortcut_init_failed",
                        format!(
                            "Fn shortcut initialization failed. Enable Input Monitoring / Accessibility and restart. Details: {}",
                            message
                        ),
                        true,
                    );
                    let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
                    inner.state.shortcut_ready = false;
                    inner.state.phase = Phase::Error;
                    log::error!("event=shortcut_init_failed reason={}", err.message);
                    shortcut_error = Some(err);
                }
            }
        }

        if let Some(err) = shortcut_error.clone() {
            emit_error(app, &err);
        }

        emit_state(app, &self.current_state());

        RuntimeInitResult {
            state: self.current_state(),
            shortcut_error,
        }
    }

    pub fn set_mic_permission(&self, app: &AppHandle, granted: bool) {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.mic_permission = if granted {
                MicPermission::Granted
            } else {
                MicPermission::Denied
            };
        }
        emit_state(app, &self.current_state());
    }

    pub fn current_state(&self) -> RuntimeState {
        self.inner
            .lock()
            .expect("runtime state mutex poisoned")
            .state
            .clone()
    }

    pub fn set_mode(&self, app: &AppHandle, mode: RecordMode) {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.mode = mode;
        }
        emit_state(app, &self.current_state());
    }

    pub fn mode(&self) -> RecordMode {
        self.inner
            .lock()
            .expect("runtime state mutex poisoned")
            .state
            .mode
    }

    pub fn handle_fn_event(&self, app: &AppHandle, is_pressed: bool) {
        let (mode, phase, should_ignore_toggle_press) = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");

            if !inner.state.shortcut_ready {
                return;
            }

            let mut ignore = false;
            if inner.state.mode == RecordMode::Toggle && is_pressed {
                let now = Instant::now();
                if let Some(last) = inner.last_toggle_press {
                    if now.duration_since(last) < Duration::from_millis(TOGGLE_DEBOUNCE_MS) {
                        ignore = true;
                    }
                }
                inner.last_toggle_press = Some(now);
            }

            (inner.state.mode, inner.state.phase.clone(), ignore)
        };

        if should_ignore_toggle_press {
            return;
        }

        match mode {
            RecordMode::PushToTalk => {
                if is_pressed && matches!(phase, Phase::Idle | Phase::Error) {
                    if let Err(err) = self.start_recording(app, "shortcut") {
                        self.publish_error(app, err);
                    }
                } else if !is_pressed && phase == Phase::Recording {
                    let controller = self.clone();
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = controller
                            .stop_recording_and_transcribe(&app_handle, "shortcut")
                            .await
                        {
                            controller.publish_error(&app_handle, err);
                        }
                    });
                }
            }
            RecordMode::Toggle => {
                if !is_pressed {
                    return;
                }

                if matches!(phase, Phase::Idle | Phase::Error) {
                    if let Err(err) = self.start_recording(app, "shortcut") {
                        self.publish_error(app, err);
                    }
                } else if phase == Phase::Recording {
                    let controller = self.clone();
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = controller
                            .stop_recording_and_transcribe(&app_handle, "shortcut")
                            .await
                        {
                            controller.publish_error(&app_handle, err);
                        }
                    });
                }
            }
        }
    }

    pub fn start_recording(&self, app: &AppHandle, source: &str) -> Result<(), RuntimeError> {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");

            if inner.state.phase == Phase::Error {
                inner.state.phase = Phase::Idle;
            }

            if inner.state.mic_permission == MicPermission::Denied {
                return Err(runtime_error(
                    "mic_permission_denied",
                    "Microphone permission is denied. Grant permission and retry.",
                    true,
                ));
            }

            if inner.state.phase != Phase::Idle {
                return Err(runtime_error(
                    "invalid_transition",
                    format!(
                        "Cannot start recording while phase is {:?}",
                        inner.state.phase
                    ),
                    true,
                ));
            }

            inner
                .audio
                .start()
                .map_err(|e| runtime_error("recording_start_failed", e, true))?;
            inner.state.phase = Phase::Recording;
        }

        log::info!("event=recording_start source={}", source);
        emit_state(app, &self.current_state());

        Ok(())
    }

    pub async fn stop_recording_and_transcribe(
        &self,
        app: &AppHandle,
        source: &str,
    ) -> Result<TranscriptionResult, RuntimeError> {
        let (captured, model_id) = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");

            if inner.state.phase != Phase::Recording {
                return Err(runtime_error(
                    "invalid_transition",
                    format!(
                        "Cannot stop recording while phase is {:?}",
                        inner.state.phase
                    ),
                    true,
                ));
            }

            inner.state.phase = Phase::Transcribing;
            let captured = inner
                .audio
                .stop()
                .map_err(|e| runtime_error("recording_stop_failed", e, true))?;

            (captured, inner.model_id.clone())
        };

        emit_state(app, &self.current_state());
        log::info!("event=recording_stop source={}", source);

        let model_id_for_task = model_id.clone();
        let transcribe_started = Instant::now();
        let transcription_text = tauri::async_runtime::spawn_blocking(move || {
            transcribe_captured_audio(captured, &model_id_for_task)
        })
        .await
        .map_err(|e| runtime_error("transcription_task_failed", e.to_string(), true))??;

        let post_processor = NoopPostProcessor;
        let final_text = post_processor.process(&transcription_text)?;

        let mut copied_to_clipboard = true;
        if let Err(e) = app.clipboard().write_text(final_text.clone()) {
            copied_to_clipboard = false;
            let clipboard_error = runtime_error(
                "clipboard_write_failed",
                format!("Transcription completed but clipboard copy failed: {}", e),
                true,
            );
            self.publish_error(app, clipboard_error);
        }

        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.phase = Phase::Idle;
        }
        emit_state(app, &self.current_state());

        let result = TranscriptionResult {
            text: final_text,
            model_id,
            duration_ms: transcribe_started.elapsed().as_millis() as u64,
            copied_to_clipboard,
        };

        log::info!(
            "event=transcription_complete source={} duration_ms={} copied_to_clipboard={}",
            source,
            result.duration_ms,
            result.copied_to_clipboard
        );

        emit_transcription(app, &result);

        Ok(result)
    }

    pub fn publish_error(&self, app: &AppHandle, err: RuntimeError) {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            if inner.audio.is_recording() {
                let _ = inner.audio.stop();
            }
            inner.state.phase = Phase::Error;
        }
        log::error!(
            "event=runtime_error code={} message={}",
            err.code,
            err.message
        );
        emit_state(app, &self.current_state());
        emit_error(app, &err);
    }
}

fn initialize_shortcut_with_timeout(
    app: AppHandle,
    timeout_ms: u64,
) -> Result<FnShortcutManager, String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = FnShortcutManager::start(app);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "Timed out while initializing Fn shortcut after {}ms",
            timeout_ms
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Fn shortcut initializer thread disconnected".to_string())
        }
    }
}

fn transcribe_captured_audio(
    captured: CapturedAudio,
    model_id: &str,
) -> Result<String, RuntimeError> {
    let min_samples = (captured.sample_rate as f32 * MIN_AUDIO_SECONDS) as usize;
    if captured.samples.len() < min_samples {
        return Err(runtime_error(
            "audio_too_short",
            "Recorded audio is too short. Hold Fn longer and try again.",
            true,
        ));
    }

    let temp_dir = ensure_temp_dir().map_err(|e| {
        runtime_error(
            "temp_dir_failed",
            format!(
                "Could not prepare temporary directory for transcription: {}",
                e
            ),
            true,
        )
    })?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f");
    let input_path = temp_dir.join(format!("recording_{}.wav", timestamp));
    let output_path = temp_dir.join(format!("transcript_{}.txt", timestamp));

    write_wav_file(&input_path, &captured).map_err(|e| {
        runtime_error(
            "wav_write_failed",
            format!("Failed to create temporary recording file: {}", e),
            true,
        )
    })?;

    let text = transcribe_file(&input_path, &output_path, model_id).map_err(|e| {
        runtime_error(
            "transcription_failed",
            format!("Rust transcriber failed: {}", e),
            true,
        )
    })?;

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err(runtime_error(
            "empty_transcript",
            "Transcription returned empty text. Try speaking more clearly and retry.",
            true,
        ));
    }

    Ok(trimmed)
}

fn ensure_temp_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("steno");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn emit_state(app: &AppHandle, state: &RuntimeState) {
    let _ = app.emit("steno://state-changed", state.clone());
}

fn emit_transcription(app: &AppHandle, result: &TranscriptionResult) {
    let _ = app.emit("steno://transcription-complete", result.clone());
}

fn emit_error(app: &AppHandle, err: &RuntimeError) {
    let _ = app.emit("steno://error", err.clone());
}

fn runtime_error(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> RuntimeError {
    RuntimeError {
        code: code.into(),
        message: message.into(),
        recoverable,
    }
}

pub fn handle_fn_hotkey_event(app: &AppHandle, is_pressed: bool) {
    if let Some(controller) = app.try_state::<RuntimeController>() {
        controller.handle_fn_event(app, is_pressed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_error_contains_fields() {
        let err = runtime_error("sample", "message", true);
        assert_eq!(err.code, "sample");
        assert_eq!(err.message, "message");
        assert!(err.recoverable);
    }

    #[test]
    fn default_mode_is_push_to_talk() {
        let runtime = RuntimeController::new();
        assert_eq!(runtime.mode(), RecordMode::PushToTalk);
    }
}
