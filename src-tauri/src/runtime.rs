use crate::audio_capture::{
    finalize_recording_wav, AudioCapture, CaptureDestinationPolicy, CaptureEngineError,
    CapturedAudio,
};
use crate::model_download::is_model_ready;
use crate::overlay_runtime::{OverlayRuntimeController, OverlayRuntimePhase};
use crate::post_process::{DefaultPostProcessor, PostProcessor};
use crate::readiness;
use crate::shortcut::{self, FnShortcutManager, ShortcutBinding};
use crate::vad::{LiveWordVad, LiveWordVadError, LIVE_WORD_VAD_SAMPLE_RATE};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use transcriber_core::{
    transcribe_file_with_config, transcriber::DEFAULT_PARAKEET_MODEL,
    RuntimeSelection as CoreRuntimeSelection, TranscriptionConfig,
};

const MIN_AUDIO_SECONDS: f32 = 0.20;
const MAX_AUDIO_SECONDS: f32 = 180.0;
const TOGGLE_DEBOUNCE_MS: u64 = 180;
const HOTKEY_INIT_TIMEOUT_MS: u64 = 5000;
const RELIABILITY_TARGET_MS: u64 = 12_000;
const RECORDING_START_TIMEOUT_MS: u64 = 3000;
const RECORDING_STOP_TIMEOUT_MS: u64 = 3000;
const RECORDING_SNAPSHOT_TIMEOUT_MS: u64 = 300;
const TRANSCRIPTION_TIMEOUT_MS: u64 = 90_000;
const OUTPUT_ACTION_TIMEOUT_MS: u64 = 2500;
const OUTPUT_ACTION_RETRY_COUNT: usize = 1;
const CLIPBOARD_RESTORE_DELAY_MS: u64 = 180;
const INTERIM_EMIT_INTERVAL_MS: u64 = 1200;
const INTERIM_TRANSCRIPTION_TIMEOUT_MS: u64 = 7000;
const INTERIM_TIMEOUT_DISABLE_THRESHOLD: u64 = 5;
const LIVE_WORD_TRANSCRIPTION_TIMEOUT_MS: u64 = 5000;
const LIVE_WORD_MIN_TRANSCRIBE_MS: u64 = 80;
const LIVE_WORD_MIN_RMS: f32 = 0.0025;
const LIVE_WORD_MIN_PEAK: f32 = 0.012;

const DEFAULT_WHISPER_FAST_MODEL: &str = "openai/whisper-tiny";
const DEFAULT_WHISPER_BALANCED_MODEL: &str = "openai/whisper-base";
const DEFAULT_WHISPER_ACCURATE_MODEL: &str = "openai/whisper-small";
const DEFAULT_PARAKEET_ONNX_MODEL: &str = "istupakov/parakeet-tdt-0.6b-v3-onnx";

const SETTINGS_FILE_NAME: &str = "settings.json";

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputMonitoringPermission {
    Unknown,
    Granted,
    Denied,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardPolicy {
    RestorePrevious,
    KeepTranscript,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStatus {
    AutoPasted,
    PasteFailed,
    CopiedOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionRuntime {
    Whisper,
    Parakeet,
    Moonshine,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MoonshineVariant {
    Tiny,
    Base,
}

impl Default for MoonshineVariant {
    fn default() -> Self {
        Self::Tiny
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelProfile {
    Fast,
    Balanced,
    Accurate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeState {
    pub phase: Phase,
    pub mode: RecordMode,
    pub shortcut_ready: bool,
    pub mic_permission: MicPermission,
    pub input_monitoring_permission: InputMonitoringPermission,
    pub clipboard_policy: ClipboardPolicy,
    pub push_to_talk_shortcut: String,
    pub toggle_shortcut: String,
    pub runtime_selection: TranscriptionRuntime,
    pub model_profile: ModelProfile,
    pub parakeet_model_id: String,
    pub moonshine_variant: MoonshineVariant,
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
    pub runtime_used: TranscriptionRuntime,
    pub model_id: String,
    pub duration_ms: u64,
    pub reliability_target_ms: u64,
    pub reliability_target_met: bool,
    pub copied_to_clipboard: bool,
    pub output_status: OutputStatus,
    pub clipboard_restored: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterimTranscriptionFrame {
    pub session_id: String,
    pub seq: u64,
    pub text: String,
    pub is_stable: bool,
    pub emitted_at_ms: u64,
}

#[derive(Debug, Clone, Default)]
struct InterimCounters {
    interim_emit_count: u64,
    interim_drop_count: u64,
    interim_timeout_count: u64,
    interim_auto_disable_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveWordOutputFrame {
    pub session_id: String,
    pub seq: u64,
    pub text: String,
    pub emitted_at_ms: u64,
}

#[derive(Debug, Clone, Default)]
struct SentenceCommitState {
    committed_text: String,
    committed_sample_count: usize,
    committed_sentence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSettings {
    mode: RecordMode,
    clipboard_policy: ClipboardPolicy,
    push_to_talk_shortcut: String,
    toggle_shortcut: String,
    runtime_selection: TranscriptionRuntime,
    model_profile: ModelProfile,
    #[serde(default = "default_parakeet_model_id")]
    parakeet_model_id: String,
    #[serde(default)]
    moonshine_variant: MoonshineVariant,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            mode: RecordMode::PushToTalk,
            clipboard_policy: ClipboardPolicy::RestorePrevious,
            push_to_talk_shortcut: shortcut::DEFAULT_PUSH_TO_TALK_SHORTCUT.to_string(),
            toggle_shortcut: shortcut::DEFAULT_TOGGLE_SHORTCUT.to_string(),
            runtime_selection: TranscriptionRuntime::Whisper,
            model_profile: ModelProfile::Balanced,
            parakeet_model_id: default_parakeet_model_id(),
            moonshine_variant: MoonshineVariant::Tiny,
        }
    }
}

fn default_parakeet_model_id() -> String {
    DEFAULT_PARAKEET_ONNX_MODEL.to_string()
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
    Snapshot {
        response: mpsc::Sender<Result<CapturedAudio, String>>,
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
                    AudioCommand::Snapshot { response } => {
                        let result = capture.snapshot().map_err(|e| e.to_string());
                        let _ = response.send(result);
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
            .recv_timeout(Duration::from_millis(RECORDING_START_TIMEOUT_MS))
            .map_err(|_| {
                format!(
                    "Timed out waiting for audio start response after {}ms",
                    RECORDING_START_TIMEOUT_MS
                )
            })?
    }

    fn stop(&self) -> Result<CapturedAudio, String> {
        let (response_tx, response_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::Stop {
                response: response_tx,
            })
            .map_err(|e| format!("Failed to send stop command to audio worker: {}", e))?;

        response_rx
            .recv_timeout(Duration::from_millis(RECORDING_STOP_TIMEOUT_MS))
            .map_err(|_| {
                format!(
                    "Timed out waiting for audio stop response after {}ms",
                    RECORDING_STOP_TIMEOUT_MS
                )
            })?
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

    fn snapshot(&self) -> Result<CapturedAudio, String> {
        let (response_tx, response_rx) = mpsc::channel();
        self.tx
            .send(AudioCommand::Snapshot {
                response: response_tx,
            })
            .map_err(|e| format!("Failed to send snapshot command to audio worker: {}", e))?;

        response_rx
            .recv_timeout(Duration::from_millis(RECORDING_SNAPSHOT_TIMEOUT_MS))
            .map_err(|_| {
                format!(
                    "Timed out waiting for audio snapshot response after {}ms",
                    RECORDING_SNAPSHOT_TIMEOUT_MS
                )
            })?
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
    audio: AudioWorker,
    shortcut_manager: Option<FnShortcutManager>,
    last_toggle_press: Option<Instant>,
    active_recording_shortcut: Option<ShortcutBinding>,
    settings_loaded: bool,
    interim_session_id: Option<String>,
    interim_seq: u64,
    interim_stop_tx: Option<mpsc::Sender<()>>,
    interim_disabled: bool,
    interim_counters: InterimCounters,
    live_word_seq: u64,
    live_word_text: String,
    live_word_vad: Option<LiveWordVad>,
    live_word_processed_samples: usize,
    sentence_commit: SentenceCommitState,
}

#[derive(Clone)]
pub struct RuntimeController {
    inner: Arc<Mutex<RuntimeInner>>,
}

impl RuntimeController {
    pub fn new() -> Self {
        let defaults = PersistedSettings::default();
        Self {
            inner: Arc::new(Mutex::new(RuntimeInner {
                state: RuntimeState {
                    phase: Phase::Idle,
                    mode: defaults.mode,
                    shortcut_ready: false,
                    mic_permission: MicPermission::Unknown,
                    input_monitoring_permission: InputMonitoringPermission::Unknown,
                    clipboard_policy: defaults.clipboard_policy,
                    push_to_talk_shortcut: defaults.push_to_talk_shortcut.clone(),
                    toggle_shortcut: defaults.toggle_shortcut.clone(),
                    runtime_selection: defaults.runtime_selection,
                    model_profile: defaults.model_profile,
                    parakeet_model_id: defaults.parakeet_model_id.clone(),
                    moonshine_variant: defaults.moonshine_variant,
                },
                audio: AudioWorker::new(),
                shortcut_manager: None,
                last_toggle_press: None,
                active_recording_shortcut: None,
                settings_loaded: false,
                interim_session_id: None,
                interim_seq: 0,
                interim_stop_tx: None,
                interim_disabled: false,
                interim_counters: InterimCounters::default(),
                live_word_seq: 0,
                live_word_text: String::new(),
                live_word_vad: None,
                live_word_processed_samples: 0,
                sentence_commit: SentenceCommitState::default(),
            })),
        }
    }

    pub fn initialize(&self, app: &AppHandle) -> RuntimeInitResult {
        let mut shortcut_error = None;

        if let Err(err) = self.ensure_settings_loaded(app) {
            log::warn!("event=settings_load_failed reason={}", err.message);
            shortcut_error = Some(err.clone());
            emit_error(app, &err);
            let _ = self.persist_settings(app);
        }

        if let Err(err) = self.reinitialize_shortcuts(app) {
            shortcut_error = Some(err.clone());
            log::error!("event=shortcut_init_failed reason={}", err.message);
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
            inner.state.mic_permission =
                map_mic_permission(readiness::permission_state_from_granted(granted));
        }
        emit_state(app, &self.current_state());
    }

    pub fn set_input_monitoring_permission(
        &self,
        app: &AppHandle,
        granted: bool,
    ) -> Result<(), RuntimeError> {
        if !granted {
            {
                let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
                inner.state.input_monitoring_permission = map_input_monitoring_permission(
                    readiness::permission_state_from_granted(false),
                );
                inner.shortcut_manager = None;
                inner.last_toggle_press = None;
                inner.active_recording_shortcut = None;
                inner.state.shortcut_ready = false;
                if inner.state.phase == Phase::Error {
                    inner.state.phase = Phase::Idle;
                }
            }

            self.emit_recoverable_error(
                app,
                readiness::input_monitoring_permission_denied_issue().code,
                format!(
                    "{} {} Manual controls remain available.",
                    readiness::input_monitoring_permission_denied_issue().message,
                    readiness::input_monitoring_permission_denied_issue().guidance
                ),
            );
            emit_state(app, &self.current_state());
            return Ok(());
        }

        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.input_monitoring_permission =
                map_input_monitoring_permission(readiness::permission_state_from_granted(true));
        }

        self.reinitialize_shortcuts(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn current_state(&self) -> RuntimeState {
        self.inner
            .lock()
            .expect("runtime state mutex poisoned")
            .state
            .clone()
    }

    fn start_interim_stream(&self, app: &AppHandle) {
        let session_id = format!("session-{}", chrono::Utc::now().timestamp_millis());
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.interim_session_id = Some(session_id.clone());
            inner.interim_seq = 0;
            inner.interim_disabled = false;
            inner.interim_counters = InterimCounters::default();
            inner.interim_stop_tx = Some(stop_tx);
            inner.live_word_seq = 0;
            inner.live_word_text.clear();
            inner.live_word_processed_samples = 0;
            if let Some(vad) = inner.live_word_vad.as_mut() {
                vad.reset();
            }
            inner.sentence_commit = SentenceCommitState::default();
        }

        let controller = self.clone();
        let app_handle = app.clone();
        thread::spawn(move || loop {
            match stop_rx.recv_timeout(Duration::from_millis(INTERIM_EMIT_INTERVAL_MS)) {
                Ok(()) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !controller.emit_interim_frame(&app_handle, &session_id) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        });
    }

    fn emit_interim_frame(&self, app: &AppHandle, session_id: &str) -> bool {
        let (
            runtime_selection,
            model_profile,
            parakeet_model_id,
            moonshine_variant,
            clipboard_policy,
            mut sentence_commit,
            captured,
        ) = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            if inner.state.phase != Phase::Recording {
                return false;
            }

            if inner.interim_disabled {
                return false;
            }

            if inner.interim_session_id.as_deref() != Some(session_id) {
                return false;
            }

            let captured = match inner.audio.snapshot() {
                Ok(audio) => audio,
                Err(_) => {
                    inner.interim_counters.interim_drop_count += 1;
                    return true;
                }
            };

            (
                inner.state.runtime_selection,
                inner.state.model_profile,
                inner.state.parakeet_model_id.clone(),
                inner.state.moonshine_variant,
                inner.state.clipboard_policy,
                inner.sentence_commit.clone(),
                captured,
            )
        };

        if !self.emit_live_word_output(app, session_id, model_profile, &captured) {
            return false;
        }

        let min_samples = (captured.sample_rate as f32 * MIN_AUDIO_SECONDS) as usize;
        if captured.samples.len() < min_samples {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            if inner.interim_session_id.as_deref() == Some(session_id) {
                inner.interim_counters.interim_drop_count += 1;
            }
            return true;
        }

        let interim_audio = CapturedAudio {
            samples: captured.samples.clone(),
            sample_rate: captured.sample_rate,
        };

        let interim_output = match transcribe_captured_audio_with_timeout(
            interim_audio,
            runtime_selection,
            model_profile,
            parakeet_model_id.clone(),
            moonshine_variant,
            INTERIM_TRANSCRIPTION_TIMEOUT_MS,
        ) {
            Ok(output) => output,
            Err(err) => {
                let mut should_disable = false;

                {
                    let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
                    if inner.interim_session_id.as_deref() != Some(session_id) {
                        return false;
                    }

                    inner.interim_counters.interim_drop_count += 1;

                    if err.code == "transcription_timeout" {
                        inner.interim_counters.interim_timeout_count += 1;
                        if inner.interim_counters.interim_timeout_count
                            >= INTERIM_TIMEOUT_DISABLE_THRESHOLD
                        {
                            inner.interim_disabled = true;
                            inner.interim_counters.interim_auto_disable_count += 1;
                            should_disable = true;
                        }
                    }
                }

                if should_disable {
                    self.emit_recoverable_error(
                        app,
                        "interim_auto_disabled",
                        "Interim preview auto-disabled due repeated timeout budget breaches."
                            .to_string(),
                    );
                    return false;
                }

                return true;
            }
        };

        if !self.is_interim_session_active(session_id) {
            return false;
        }

        let detected_sentence_count = count_sentence_stoppers(&interim_output.text);
        if detected_sentence_count > sentence_commit.committed_sentence_count {
            if let Some(segment_audio) = slice_captured_audio(
                &captured,
                sentence_commit.committed_sample_count,
                captured.samples.len(),
            ) {
                let segment_result = transcribe_captured_audio_with_timeout(
                    segment_audio,
                    runtime_selection,
                    model_profile,
                    parakeet_model_id.clone(),
                    moonshine_variant,
                    TRANSCRIPTION_TIMEOUT_MS,
                );

                if !self.is_interim_session_active(session_id) {
                    return false;
                }

                match segment_result {
                    Ok(segment_output) => {
                        let post_processor = DefaultPostProcessor;
                        match post_processor.process(&segment_output.text) {
                            Ok(segment_text) => {
                                if !segment_text.is_empty() {
                                    self.commit_incremental_chunk(
                                        app,
                                        &segment_text,
                                        clipboard_policy,
                                    );
                                    append_sentence_commit_text(
                                        &mut sentence_commit.committed_text,
                                        &segment_text,
                                    );
                                    sentence_commit.committed_sample_count = captured.samples.len();
                                    sentence_commit.committed_sentence_count =
                                        detected_sentence_count;
                                }
                            }
                            Err(err) => {
                                self.emit_recoverable_error(
                                    app,
                                    err.code.as_str(),
                                    format!(
                                        "Incremental segment post-processing failed: {}",
                                        err.message
                                    ),
                                );
                            }
                        }
                    }
                    Err(err) => {
                        self.emit_recoverable_error(
                            app,
                            err.code.as_str(),
                            format!("Incremental segment transcription failed: {}", err.message),
                        );
                    }
                }
            }
        }

        let tail_text = tail_after_sentence_count(
            &interim_output.text,
            sentence_commit.committed_sentence_count,
        );
        let preview_text = merge_committed_with_tail(&sentence_commit.committed_text, &tail_text);

        let frame = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            if inner.state.phase != Phase::Recording {
                return false;
            }
            if inner.interim_session_id.as_deref() != Some(session_id) {
                return false;
            }

            inner.interim_seq += 1;
            inner.interim_counters.interim_emit_count += 1;
            inner.interim_counters.interim_timeout_count = 0;
            inner.sentence_commit = sentence_commit;

            InterimTranscriptionFrame {
                session_id: session_id.to_string(),
                seq: inner.interim_seq,
                text: preview_text,
                is_stable: false,
                emitted_at_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            }
        };

        emit_interim_transcription(app, &frame);
        true
    }

    fn is_interim_session_active(&self, session_id: &str) -> bool {
        let inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner.state.phase == Phase::Recording
            && inner.interim_session_id.as_deref() == Some(session_id)
    }

    fn commit_incremental_chunk(
        &self,
        app: &AppHandle,
        text: &str,
        clipboard_policy: ClipboardPolicy,
    ) {
        let chunk_text = text.trim();
        if chunk_text.is_empty() {
            return;
        }

        let mut previous_clipboard_text: Option<String> = None;
        if clipboard_policy == ClipboardPolicy::RestorePrevious {
            match read_clipboard_text_with_timeout(app) {
                Ok(text) => {
                    previous_clipboard_text = Some(text);
                }
                Err(action_failure) => {
                    self.emit_recoverable_error(
                        app,
                        map_output_failure_code(
                            "incremental_clipboard_snapshot_failed",
                            &action_failure,
                        ),
                        format!(
                            "Incremental commit clipboard snapshot failed: {}",
                            action_failure.message
                        ),
                    );
                }
            }
        }

        if let Err(action_failure) = write_clipboard_text_with_retry(app, chunk_text) {
            self.emit_recoverable_error(
                app,
                map_output_failure_code("incremental_clipboard_write_failed", &action_failure),
                format!(
                    "Incremental commit clipboard copy failed: {}",
                    action_failure.message
                ),
            );
            return;
        }

        if let Err(action_failure) = trigger_system_paste_with_retry() {
            self.emit_recoverable_error(
                app,
                map_output_failure_code("incremental_auto_paste_failed", &action_failure),
                format!(
                    "Incremental commit auto-paste failed: {}",
                    action_failure.message
                ),
            );
        }

        if clipboard_policy == ClipboardPolicy::RestorePrevious {
            if let Some(previous_text) = previous_clipboard_text {
                std::thread::sleep(Duration::from_millis(CLIPBOARD_RESTORE_DELAY_MS));
                if let Err(action_failure) = write_clipboard_text_with_retry(app, &previous_text) {
                    self.emit_recoverable_error(
                        app,
                        map_output_failure_code(
                            "incremental_clipboard_restore_failed",
                            &action_failure,
                        ),
                        format!(
                            "Incremental commit clipboard restore failed: {}",
                            action_failure.message
                        ),
                    );
                }
            }
        }
    }

    fn emit_live_word_output(
        &self,
        app: &AppHandle,
        session_id: &str,
        fallback_profile: ModelProfile,
        captured: &CapturedAudio,
    ) -> bool {
        let (candidate_ranges, mut next_text) = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            if inner.state.phase != Phase::Recording {
                return false;
            }
            if inner.interim_session_id.as_deref() != Some(session_id) {
                return false;
            }

            if inner.live_word_processed_samples > captured.samples.len() {
                inner.live_word_processed_samples = 0;
                if let Some(vad) = inner.live_word_vad.as_mut() {
                    vad.reset();
                }
            }

            let new_samples = &captured.samples[inner.live_word_processed_samples..];
            inner.live_word_processed_samples = captured.samples.len();
            let vad_input_samples = resample_for_live_word_vad(new_samples, captured.sample_rate);

            let candidate_ranges: Vec<(usize, usize)> =
                if let Some(vad) = inner.live_word_vad.as_mut() {
                    match vad.push_samples(&vad_input_samples) {
                        Ok(segments) => segments
                            .into_iter()
                            .map(|(start, end)| {
                                (
                                    map_vad_index_to_source_sample(start, captured.sample_rate),
                                    map_vad_index_to_source_sample(end, captured.sample_rate),
                                )
                            })
                            .collect(),
                        Err(err) => {
                            self.emit_recoverable_error(
                                app,
                                "live_word_vad_init_failed",
                                format!("Live word VAD processing failed: {}", err),
                            );
                            inner.interim_disabled = true;
                            return false;
                        }
                    }
                } else {
                    self.emit_recoverable_error(
                        app,
                        "live_word_vad_init_failed",
                        "Live word VAD is not initialized for the recording session.".to_string(),
                    );
                    inner.interim_disabled = true;
                    return false;
                };

            (candidate_ranges, inner.live_word_text.clone())
        };

        if candidate_ranges.is_empty() {
            return true;
        }

        let Some(interim_profile) = resolve_live_word_profile(fallback_profile) else {
            return true;
        };

        let min_word_samples =
            ((captured.sample_rate as u64 * LIVE_WORD_MIN_TRANSCRIBE_MS) / 1000).max(1) as usize;

        for (start_idx, end_idx) in candidate_ranges {
            if end_idx <= start_idx {
                continue;
            }
            let bounded_start = start_idx.min(captured.samples.len());
            let bounded_end = end_idx.min(captured.samples.len());
            if bounded_end <= bounded_start {
                continue;
            }
            if bounded_end - bounded_start < min_word_samples {
                continue;
            }
            let candidate_samples = &captured.samples[bounded_start..bounded_end];
            if !is_likely_live_word_candidate(candidate_samples) {
                continue;
            }

            let candidate_audio = CapturedAudio {
                samples: candidate_samples.to_vec(),
                sample_rate: captured.sample_rate,
            };

            let candidate_text = match transcribe_captured_audio_with_timeout(
                candidate_audio,
                TranscriptionRuntime::Whisper,
                interim_profile,
                String::new(),
                MoonshineVariant::Tiny,
                LIVE_WORD_TRANSCRIPTION_TIMEOUT_MS,
            ) {
                Ok(output) => output.text,
                Err(_) => continue,
            };

            let post_processor = DefaultPostProcessor;
            let normalized_candidate = match post_processor.process(&candidate_text) {
                Ok(text) => text,
                Err(_) => continue,
            };
            if is_low_information_live_word_text(&normalized_candidate) {
                continue;
            }

            append_live_word_text(&mut next_text, &normalized_candidate);
        }

        if next_text.is_empty() {
            return true;
        }

        let frame = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            if inner.state.phase != Phase::Recording {
                return false;
            }
            if inner.interim_session_id.as_deref() != Some(session_id) {
                return false;
            }
            if inner.live_word_text == next_text {
                return true;
            }

            inner.live_word_seq += 1;
            inner.live_word_text = next_text.clone();

            LiveWordOutputFrame {
                session_id: session_id.to_string(),
                seq: inner.live_word_seq,
                text: next_text,
                emitted_at_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            }
        };

        emit_live_word_output_frame(app, &frame);
        true
    }

    pub fn set_mode(&self, app: &AppHandle, mode: RecordMode) -> Result<(), RuntimeError> {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.mode = mode;
        }

        self.persist_settings(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn set_hotkey_bindings(
        &self,
        app: &AppHandle,
        push: String,
        toggle: String,
    ) -> Result<(), RuntimeError> {
        let bindings =
            shortcut::normalize_shortcut_bindings(&push, &toggle).map_err(map_shortcut_error)?;

        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.push_to_talk_shortcut = bindings.push_to_talk;
            inner.state.toggle_shortcut = bindings.toggle;
        }

        self.persist_settings(app)?;
        self.reinitialize_shortcuts(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn suspend_shortcuts_for_capture(&self) -> bool {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        let had_shortcut_manager = inner.shortcut_manager.is_some();
        inner.shortcut_manager = None;
        inner.last_toggle_press = None;
        inner.active_recording_shortcut = None;
        had_shortcut_manager
    }

    pub fn resume_shortcuts_after_capture(
        &self,
        app: &AppHandle,
        should_resume: bool,
    ) -> Result<(), RuntimeError> {
        if !should_resume {
            return Ok(());
        }
        self.reinitialize_shortcuts(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn set_runtime_selection(
        &self,
        app: &AppHandle,
        selection: TranscriptionRuntime,
    ) -> Result<(), RuntimeError> {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.runtime_selection = selection;
        }
        self.persist_settings(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn set_model_profile(
        &self,
        app: &AppHandle,
        profile: ModelProfile,
    ) -> Result<(), RuntimeError> {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.model_profile = profile;
        }
        self.persist_settings(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn set_parakeet_model_id(
        &self,
        app: &AppHandle,
        model_id: String,
    ) -> Result<(), RuntimeError> {
        let normalized = model_id.trim();
        if normalized.is_empty() {
            return Err(runtime_error(
                "invalid_model_id",
                "Parakeet model ID cannot be empty.",
                true,
            ));
        }

        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.parakeet_model_id = normalized.to_string();
        }
        self.persist_settings(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn set_moonshine_variant(
        &self,
        app: &AppHandle,
        variant: MoonshineVariant,
    ) -> Result<(), RuntimeError> {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.moonshine_variant = variant;
        }
        self.persist_settings(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn mode(&self) -> RecordMode {
        self.inner
            .lock()
            .expect("runtime state mutex poisoned")
            .state
            .mode
    }

    pub fn set_clipboard_policy(
        &self,
        app: &AppHandle,
        policy: ClipboardPolicy,
    ) -> Result<(), RuntimeError> {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.clipboard_policy = policy;
        }
        self.persist_settings(app)?;
        emit_state(app, &self.current_state());
        Ok(())
    }

    pub fn handle_active_shortcut_event(
        &self,
        app: &AppHandle,
        shortcut_binding: ShortcutBinding,
        is_pressed: bool,
    ) {
        let (phase, active_recording_shortcut, should_ignore_toggle_press) = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");

            if !inner.state.shortcut_ready {
                return;
            }

            let mut ignore = false;
            if shortcut_binding == ShortcutBinding::Toggle && is_pressed {
                let now = Instant::now();
                if let Some(last) = inner.last_toggle_press {
                    if now.duration_since(last) < Duration::from_millis(TOGGLE_DEBOUNCE_MS) {
                        ignore = true;
                    }
                }
                inner.last_toggle_press = Some(now);
            }

            (
                inner.state.phase.clone(),
                inner.active_recording_shortcut,
                ignore,
            )
        };

        if should_ignore_toggle_press {
            return;
        }

        match shortcut_binding {
            ShortcutBinding::PushToTalk => {
                if is_pressed && matches!(phase, Phase::Idle | Phase::Error) {
                    if let Err(err) = self.start_recording_with_trigger(
                        app,
                        "shortcut_push_to_talk",
                        Some(ShortcutBinding::PushToTalk),
                    ) {
                        self.publish_error(app, err);
                    }
                } else if !is_pressed
                    && phase == Phase::Recording
                    && active_recording_shortcut == Some(ShortcutBinding::PushToTalk)
                {
                    let controller = self.clone();
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = controller
                            .stop_recording_and_transcribe(&app_handle, "shortcut_push_to_talk")
                            .await
                        {
                            controller.publish_error(&app_handle, err);
                        }
                    });
                }
            }
            ShortcutBinding::Toggle => {
                if !is_pressed {
                    return;
                }

                if matches!(phase, Phase::Idle | Phase::Error) {
                    if let Err(err) = self.start_recording_with_trigger(
                        app,
                        "shortcut_toggle",
                        Some(ShortcutBinding::Toggle),
                    ) {
                        self.publish_error(app, err);
                    }
                } else if phase == Phase::Recording
                    && active_recording_shortcut != Some(ShortcutBinding::PushToTalk)
                {
                    let controller = self.clone();
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = controller
                            .stop_recording_and_transcribe(&app_handle, "shortcut_toggle")
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
        self.start_recording_with_trigger(app, source, None)
    }

    fn start_recording_with_trigger(
        &self,
        app: &AppHandle,
        source: &str,
        shortcut_binding: Option<ShortcutBinding>,
    ) -> Result<(), RuntimeError> {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");

            if inner.state.phase == Phase::Error {
                inner.state.phase = Phase::Idle;
            }

            readiness::ensure_recording_start_allowed(map_readiness_permission_from_mic(
                inner.state.mic_permission,
            ))
            .map_err(map_readiness_issue)?;

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
        }

        let live_word_vad = initialize_live_word_vad(app)?;

        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
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

            inner.audio.start().map_err(map_recording_start_error)?;
            inner.live_word_vad = Some(live_word_vad);
            inner.live_word_processed_samples = 0;
            inner.state.phase = Phase::Recording;
            inner.active_recording_shortcut = shortcut_binding;
        }

        self.start_interim_stream(app);
        log::info!("event=recording_start source={}", source);
        emit_state(app, &self.current_state());

        Ok(())
    }

    pub async fn stop_recording_and_transcribe(
        &self,
        app: &AppHandle,
        source: &str,
    ) -> Result<TranscriptionResult, RuntimeError> {
        let (
            captured,
            runtime_selection,
            model_profile,
            parakeet_model_id,
            moonshine_variant,
            clipboard_policy,
            mut sentence_commit,
        ) = {
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

            stop_interim_stream_locked(&mut inner);
            inner.state.phase = Phase::Transcribing;
            inner.active_recording_shortcut = None;
            let captured = inner.audio.stop().map_err(map_recording_stop_error)?;

            (
                captured,
                inner.state.runtime_selection,
                inner.state.model_profile,
                inner.state.parakeet_model_id.clone(),
                inner.state.moonshine_variant,
                inner.state.clipboard_policy,
                inner.sentence_commit.clone(),
            )
        };

        emit_state(app, &self.current_state());
        log::info!("event=recording_stop source={}", source);

        let transcribe_started = Instant::now();
        let tail_audio = slice_captured_audio(
            &captured,
            sentence_commit.committed_sample_count,
            captured.samples.len(),
        )
        .filter(|audio| {
            let min_samples = (audio.sample_rate as f32 * MIN_AUDIO_SECONDS) as usize;
            audio.samples.len() >= min_samples
        });

        if tail_audio.is_none() && sentence_commit.committed_text.is_empty() {
            self.set_phase_idle(app);
            return Err(runtime_error(
                "audio_too_short",
                "Recorded audio is too short. Hold the shortcut longer and retry.",
                true,
            ));
        }

        let mut runtime_used = runtime_selection;
        let mut model_id = resolve_model_id(
            runtime_selection,
            model_profile,
            &parakeet_model_id,
            moonshine_variant,
        );
        let mut tail_text = String::new();

        if let Some(tail_audio) = tail_audio {
            let tail_parakeet_model_id = parakeet_model_id.clone();
            let tail_transcription = match tauri::async_runtime::spawn_blocking(move || {
                transcribe_captured_audio_with_timeout(
                    tail_audio,
                    runtime_selection,
                    model_profile,
                    tail_parakeet_model_id,
                    moonshine_variant,
                    TRANSCRIPTION_TIMEOUT_MS,
                )
            })
            .await
            {
                Ok(result) => match result {
                    Ok(transcription) => transcription,
                    Err(err) => {
                        self.set_phase_idle(app);
                        return Err(err);
                    }
                },
                Err(err) => {
                    self.set_phase_idle(app);
                    return Err(runtime_error(
                        "transcription_task_failed",
                        err.to_string(),
                        true,
                    ));
                }
            };

            let post_processor = DefaultPostProcessor;
            tail_text = match post_processor.process(&tail_transcription.text) {
                Ok(text) => text,
                Err(err) => {
                    self.set_phase_idle(app);
                    return Err(err);
                }
            };
            runtime_used = tail_transcription.runtime_used;
            model_id = tail_transcription.model_id;
        }

        if !tail_text.is_empty() {
            sentence_commit.committed_sample_count = captured.samples.len();
            append_sentence_commit_text(&mut sentence_commit.committed_text, &tail_text);
        }

        let final_text = sentence_commit.committed_text;
        let duration_ms = transcribe_started.elapsed().as_millis() as u64;
        let reliability_target_met = duration_ms <= RELIABILITY_TARGET_MS;

        let mut previous_clipboard_text: Option<String> = None;
        let mut copied_to_clipboard = false;
        let output_status;
        let mut clipboard_restored = None;
        let mut focus_anchor: Option<String> = None;

        if tail_text.is_empty() {
            output_status = OutputStatus::CopiedOnly;
        } else {
            match read_frontmost_app_name_with_timeout() {
                Ok(name) => {
                    focus_anchor = Some(name);
                }
                Err(action_failure) => {
                    self.emit_recoverable_error(
                        app,
                        map_output_failure_code("focus_anchor_failed", &action_failure),
                        format!(
                            "Transcription completed but focus anchor capture failed: {}",
                            action_failure.message
                        ),
                    );
                }
            }

            if clipboard_policy == ClipboardPolicy::RestorePrevious {
                match read_clipboard_text_with_timeout(app) {
                    Ok(text) => {
                        previous_clipboard_text = Some(text);
                    }
                    Err(action_failure) => {
                        self.emit_recoverable_error(
                            app,
                            map_output_failure_code("clipboard_snapshot_failed", &action_failure),
                            format!(
                                "Transcription completed but clipboard snapshot failed: {}",
                                action_failure.message
                            ),
                        );
                    }
                }
            }

            if let Err(action_failure) = write_clipboard_text_with_retry(app, &tail_text) {
                output_status = OutputStatus::PasteFailed;
                self.emit_recoverable_error(
                    app,
                    map_output_failure_code("clipboard_write_failed", &action_failure),
                    format!(
                        "Transcription completed but clipboard copy failed: {}",
                        action_failure.message
                    ),
                );
            } else {
                copied_to_clipboard = true;

                let can_paste = match focus_anchor.as_deref() {
                    Some(anchor_name) => match read_frontmost_app_name_with_timeout() {
                        Ok(current_name) => {
                            if current_name == anchor_name {
                                true
                            } else {
                                self.emit_recoverable_error(
                                    app,
                                    "focus_drift_detected",
                                    format!(
                                        "Auto-paste skipped because focus changed from '{}' to '{}'.",
                                        anchor_name, current_name
                                    ),
                                );
                                false
                            }
                        }
                        Err(action_failure) => {
                            self.emit_recoverable_error(
                                app,
                                map_output_failure_code("focus_recheck_failed", &action_failure),
                                format!(
                                    "Auto-paste skipped because focus recheck failed: {}",
                                    action_failure.message
                                ),
                            );
                            false
                        }
                    },
                    None => true,
                };

                if can_paste {
                    match trigger_system_paste_with_retry() {
                        Ok(()) => {
                            output_status = OutputStatus::AutoPasted;
                        }
                        Err(action_failure) => {
                            output_status = OutputStatus::CopiedOnly;
                            self.emit_recoverable_error(
                                app,
                                map_output_failure_code("auto_paste_failed", &action_failure),
                                format!(
                                    "Transcription completed but auto-paste failed: {}",
                                    action_failure.message
                                ),
                            );
                        }
                    }
                } else {
                    output_status = OutputStatus::CopiedOnly;
                    self.emit_recoverable_error(
                        app,
                        "auto_paste_skipped_for_safety",
                        "Auto-paste skipped to avoid writing in the wrong app.".to_string(),
                    );
                }

                if clipboard_policy == ClipboardPolicy::RestorePrevious {
                    if let Some(previous_text) = previous_clipboard_text {
                        std::thread::sleep(Duration::from_millis(CLIPBOARD_RESTORE_DELAY_MS));

                        match write_clipboard_text_with_retry(app, &previous_text) {
                            Ok(()) => {
                                clipboard_restored = Some(true);
                            }
                            Err(action_failure) => {
                                clipboard_restored = Some(false);
                                self.emit_recoverable_error(
                                    app,
                                    map_output_failure_code(
                                        "clipboard_restore_failed",
                                        &action_failure,
                                    ),
                                    format!(
                                        "Transcription completed but clipboard restore failed: {}",
                                        action_failure.message
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }

        self.set_phase_idle(app);

        if !reliability_target_met {
            self.emit_recoverable_error(
                app,
                "latency_target_missed",
                format!(
                    "Transcription completed in {}ms, above the target of {}ms.",
                    duration_ms, RELIABILITY_TARGET_MS
                ),
            );
        }

        let result = TranscriptionResult {
            text: final_text,
            runtime_used,
            model_id,
            duration_ms,
            reliability_target_ms: RELIABILITY_TARGET_MS,
            reliability_target_met,
            copied_to_clipboard,
            output_status,
            clipboard_restored,
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
            stop_interim_stream_locked(&mut inner);
            if inner.audio.is_recording() {
                let _ = inner.audio.stop();
            }
            inner.active_recording_shortcut = None;
            inner.sentence_commit = SentenceCommitState::default();
            if err.recoverable {
                inner.state.phase = Phase::Idle;
            } else {
                inner.state.phase = Phase::Error;
            }
        }
        log::error!(
            "event=runtime_error code={} message={}",
            err.code,
            err.message
        );
        emit_state(app, &self.current_state());
        emit_error(app, &err);
    }

    fn set_phase_idle(&self, app: &AppHandle) {
        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            stop_interim_stream_locked(&mut inner);
            inner.state.phase = Phase::Idle;
            inner.active_recording_shortcut = None;
            inner.sentence_commit = SentenceCommitState::default();
        }
        emit_state(app, &self.current_state());
    }

    fn emit_recoverable_error(&self, app: &AppHandle, code: &str, message: String) {
        let err = runtime_error(code, message, true);
        log::warn!(
            "event=runtime_warning code={} message={}",
            err.code,
            err.message
        );
        emit_error(app, &err);
    }

    fn ensure_settings_loaded(&self, app: &AppHandle) -> Result<(), RuntimeError> {
        let is_loaded = {
            let inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.settings_loaded
        };

        if is_loaded {
            return Ok(());
        }

        let mut settings = read_persisted_settings(app)?;
        let bindings = shortcut::normalize_shortcut_bindings(
            &settings.push_to_talk_shortcut,
            &settings.toggle_shortcut,
        )
        .map_err(map_shortcut_error)?;

        settings.push_to_talk_shortcut = bindings.push_to_talk;
        settings.toggle_shortcut = bindings.toggle;

        {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.state.mode = settings.mode;
            inner.state.clipboard_policy = settings.clipboard_policy;
            inner.state.push_to_talk_shortcut = settings.push_to_talk_shortcut;
            inner.state.toggle_shortcut = settings.toggle_shortcut;
            inner.state.runtime_selection = settings.runtime_selection;
            inner.state.model_profile = settings.model_profile;
            inner.state.parakeet_model_id = settings.parakeet_model_id;
            inner.settings_loaded = true;
        }

        self.persist_settings(app)?;
        Ok(())
    }

    fn reinitialize_shortcuts(&self, app: &AppHandle) -> Result<(), RuntimeError> {
        let (push_shortcut, toggle_shortcut, input_monitoring_permission) = {
            let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
            inner.shortcut_manager = None;
            inner.last_toggle_press = None;
            inner.state.shortcut_ready = false;
            inner.active_recording_shortcut = None;
            (
                inner.state.push_to_talk_shortcut.clone(),
                inner.state.toggle_shortcut.clone(),
                inner.state.input_monitoring_permission,
            )
        };

        readiness::ensure_shortcut_registration_allowed(
            map_readiness_permission_from_input_monitoring(input_monitoring_permission),
        )
        .map_err(map_readiness_issue)?;

        let bindings = shortcut::normalize_shortcut_bindings(&push_shortcut, &toggle_shortcut)
            .map_err(map_shortcut_error)?;

        let shortcut_manager = shortcut::initialize_shortcuts_with_timeout(
            app.clone(),
            HOTKEY_INIT_TIMEOUT_MS,
            &bindings,
            handle_active_hotkey_event,
        )
        .map_err(map_shortcut_error)?;

        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner.shortcut_manager = Some(shortcut_manager);
        inner.state.shortcut_ready = true;
        if inner.state.phase == Phase::Error {
            inner.state.phase = Phase::Idle;
        }
        log::info!(
            "event=shortcuts_initialized push_to_talk={} toggle={}",
            push_shortcut,
            toggle_shortcut
        );
        Ok(())
    }

    fn persist_settings(&self, app: &AppHandle) -> Result<(), RuntimeError> {
        let settings = self.snapshot_persisted_settings();
        let path = settings_path(app)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                runtime_error(
                    "settings_persist_failed",
                    format!("Failed to prepare settings directory: {}", e),
                    true,
                )
            })?;
        }

        let serialized = serde_json::to_string_pretty(&settings).map_err(|e| {
            runtime_error(
                "settings_serialize_failed",
                format!("Failed to serialize settings: {}", e),
                true,
            )
        })?;

        fs::write(path, serialized).map_err(|e| {
            runtime_error(
                "settings_persist_failed",
                format!("Failed to write settings file: {}", e),
                true,
            )
        })?;

        Ok(())
    }

    fn snapshot_persisted_settings(&self) -> PersistedSettings {
        let inner = self.inner.lock().expect("runtime state mutex poisoned");
        PersistedSettings {
            mode: inner.state.mode,
            clipboard_policy: inner.state.clipboard_policy,
            push_to_talk_shortcut: inner.state.push_to_talk_shortcut.clone(),
            toggle_shortcut: inner.state.toggle_shortcut.clone(),
            runtime_selection: inner.state.runtime_selection,
            model_profile: inner.state.model_profile,
            parakeet_model_id: inner.state.parakeet_model_id.clone(),
            moonshine_variant: inner.state.moonshine_variant,
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, RuntimeError> {
    let config_dir = app.path().app_config_dir().map_err(|e| {
        runtime_error(
            "settings_path_failed",
            format!("Could not resolve app config path: {}", e),
            true,
        )
    })?;

    Ok(config_dir.join(SETTINGS_FILE_NAME))
}

fn read_persisted_settings(app: &AppHandle) -> Result<PersistedSettings, RuntimeError> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(PersistedSettings::default());
    }

    let raw = fs::read_to_string(path).map_err(|e| {
        runtime_error(
            "settings_read_failed",
            format!("Failed to read settings file: {}", e),
            true,
        )
    })?;

    serde_json::from_str::<PersistedSettings>(&raw).map_err(|e| {
        runtime_error(
            "settings_parse_failed",
            format!("Settings file is invalid JSON: {}", e),
            true,
        )
    })
}

fn map_mic_permission(permission: readiness::PermissionState) -> MicPermission {
    match permission {
        readiness::PermissionState::Unknown => MicPermission::Unknown,
        readiness::PermissionState::Granted => MicPermission::Granted,
        readiness::PermissionState::Denied => MicPermission::Denied,
    }
}

fn map_input_monitoring_permission(
    permission: readiness::PermissionState,
) -> InputMonitoringPermission {
    match permission {
        readiness::PermissionState::Unknown => InputMonitoringPermission::Unknown,
        readiness::PermissionState::Granted => InputMonitoringPermission::Granted,
        readiness::PermissionState::Denied => InputMonitoringPermission::Denied,
    }
}

fn map_readiness_permission_from_mic(permission: MicPermission) -> readiness::PermissionState {
    match permission {
        MicPermission::Unknown => readiness::PermissionState::Unknown,
        MicPermission::Granted => readiness::PermissionState::Granted,
        MicPermission::Denied => readiness::PermissionState::Denied,
    }
}

fn map_readiness_permission_from_input_monitoring(
    permission: InputMonitoringPermission,
) -> readiness::PermissionState {
    match permission {
        InputMonitoringPermission::Unknown => readiness::PermissionState::Unknown,
        InputMonitoringPermission::Granted => readiness::PermissionState::Granted,
        InputMonitoringPermission::Denied => readiness::PermissionState::Denied,
    }
}

fn map_readiness_issue(issue: readiness::ReadinessIssue) -> RuntimeError {
    runtime_error(
        issue.code,
        format!("{} {}", issue.message, issue.guidance),
        true,
    )
}

fn map_shortcut_error(err: shortcut::ShortcutBackendError) -> RuntimeError {
    runtime_error(
        err.code.as_str(),
        format!("{} {}", err.message, err.code.guidance()),
        true,
    )
}

fn map_recording_start_error(message: String) -> RuntimeError {
    if message.contains("Timed out waiting for audio start response") {
        return runtime_error(
            "recording_start_timeout",
            format!(
                "Recording did not start within {}ms. {}",
                RECORDING_START_TIMEOUT_MS, message
            ),
            true,
        );
    }

    runtime_error("recording_start_failed", message, true)
}

fn map_recording_stop_error(message: String) -> RuntimeError {
    if message.contains("Timed out waiting for audio stop response") {
        return runtime_error(
            "recording_stop_timeout",
            format!(
                "Recording did not stop within {}ms. {}",
                RECORDING_STOP_TIMEOUT_MS, message
            ),
            true,
        );
    }

    runtime_error("recording_stop_failed", message, true)
}

fn map_capture_finalize_error(err: CaptureEngineError) -> RuntimeError {
    match err {
        CaptureEngineError::MissingCustomPath
        | CaptureEngineError::MissingAppDataPath
        | CaptureEngineError::DestinationCreateFailed { .. }
        | CaptureEngineError::DestinationNotWritable { .. } => {
            runtime_error("output_destination_invalid", err.to_string(), true)
        }
        CaptureEngineError::WavCreateFailed { .. }
        | CaptureEngineError::WavSampleWriteFailed(_)
        | CaptureEngineError::WavFinalizeFailed(_) => {
            runtime_error("wav_write_failed", err.to_string(), true)
        }
        _ => runtime_error("recording_finalize_failed", err.to_string(), true),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActionFailureKind {
    Timeout,
    Failed,
}

#[derive(Debug, Clone)]
struct ActionFailure {
    kind: ActionFailureKind,
    message: String,
}

fn map_output_failure_code<'a>(default_code: &'a str, failure: &ActionFailure) -> &'a str {
    if failure.kind == ActionFailureKind::Timeout {
        "output_action_timeout"
    } else {
        default_code
    }
}

#[derive(Debug, Clone)]
struct TranscriptionOutput {
    text: String,
    model_id: String,
    runtime_used: TranscriptionRuntime,
}

fn transcribe_captured_audio_with_timeout(
    captured: CapturedAudio,
    runtime_selection: TranscriptionRuntime,
    model_profile: ModelProfile,
    parakeet_model_id: String,
    moonshine_variant: MoonshineVariant,
    timeout_ms: u64,
) -> Result<TranscriptionOutput, RuntimeError> {
    let result: Result<Result<TranscriptionOutput, RuntimeError>, ActionFailure> =
        run_action_with_timeout(
            timeout_ms,
            move || {
                Ok(transcribe_captured_audio(
                    captured,
                    runtime_selection,
                    model_profile,
                    parakeet_model_id,
                    moonshine_variant,
                ))
            },
            "transcription",
        );

    match result {
        Ok(transcription_result) => transcription_result,
        Err(action_failure) => {
            if action_failure.kind == ActionFailureKind::Timeout {
                Err(runtime_error(
                    "transcription_timeout",
                    format!(
                        "Transcription exceeded {}ms and was stopped.",
                        TRANSCRIPTION_TIMEOUT_MS
                    ),
                    true,
                ))
            } else {
                Err(runtime_error(
                    "transcription_task_failed",
                    format!("Transcription task failed: {}", action_failure.message),
                    true,
                ))
            }
        }
    }
}

fn transcribe_captured_audio(
    captured: CapturedAudio,
    runtime_selection: TranscriptionRuntime,
    model_profile: ModelProfile,
    parakeet_model_id: String,
    moonshine_variant: MoonshineVariant,
) -> Result<TranscriptionOutput, RuntimeError> {
    let min_samples = (captured.sample_rate as f32 * MIN_AUDIO_SECONDS) as usize;
    if captured.samples.len() < min_samples {
        return Err(runtime_error(
            "audio_too_short",
            "Recorded audio is too short. Hold the shortcut longer and retry.",
            true,
        ));
    }

    let max_samples = (captured.sample_rate as f32 * MAX_AUDIO_SECONDS) as usize;
    if captured.samples.len() > max_samples {
        return Err(runtime_error(
            "audio_too_long",
            format!(
                "Recording exceeds {:.0} seconds. Stop earlier and retry.",
                MAX_AUDIO_SECONDS
            ),
            true,
        ));
    }

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f");
    let artifacts = finalize_recording_wav(
        &captured,
        &timestamp.to_string(),
        CaptureDestinationPolicy::Temp,
        None,
        None,
    )
    .map_err(map_capture_finalize_error)?;
    let input_path = artifacts.recording_wav_path;
    let output_path = artifacts.transcript_output_path;

    let model_id = resolve_model_id(
        runtime_selection,
        model_profile,
        &parakeet_model_id,
        moonshine_variant,
    );
    let has_local_parakeet_model = runtime_selection == TranscriptionRuntime::Parakeet
        && std::path::Path::new(&model_id).is_dir();
    if !has_local_parakeet_model && !is_model_ready(&model_id) {
        let runtime_label = match runtime_selection {
            TranscriptionRuntime::Whisper => "Whisper",
            TranscriptionRuntime::Parakeet => "Parakeet",
            TranscriptionRuntime::Moonshine => "Moonshine",
        };
        return Err(runtime_error(
            "model_not_ready",
            format!(
                "Selected {} model is not downloaded. Download it in Models section.",
                runtime_label
            ),
            true,
        ));
    }

    let config = TranscriptionConfig {
        runtime: map_runtime_selection(runtime_selection),
        model_id: Some(model_id.clone()),
    };

    let text = transcribe_file_with_config(&input_path, &output_path, &config).map_err(|e| {
        runtime_error(
            "transcription_failed",
            format!("Rust transcriber failed: {:#}", e),
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

    Ok(TranscriptionOutput {
        text: trimmed,
        model_id,
        runtime_used: runtime_selection,
    })
}

fn map_runtime_selection(selection: TranscriptionRuntime) -> CoreRuntimeSelection {
    match selection {
        TranscriptionRuntime::Whisper => CoreRuntimeSelection::Whisper,
        TranscriptionRuntime::Parakeet => CoreRuntimeSelection::Parakeet,
        TranscriptionRuntime::Moonshine => CoreRuntimeSelection::Moonshine,
    }
}

fn resolve_model_id(
    selection: TranscriptionRuntime,
    profile: ModelProfile,
    parakeet_model_id: &str,
    moonshine_variant: MoonshineVariant,
) -> String {
    match selection {
        TranscriptionRuntime::Moonshine => match moonshine_variant {
            MoonshineVariant::Tiny => "moonshine-tiny".to_string(),
            MoonshineVariant::Base => "moonshine-base".to_string(),
        },
        TranscriptionRuntime::Parakeet => {
            let normalized = parakeet_model_id.trim();
            if normalized.is_empty() {
                DEFAULT_PARAKEET_MODEL.to_string()
            } else {
                normalized.to_string()
            }
        }
        TranscriptionRuntime::Whisper => match profile {
            ModelProfile::Fast => DEFAULT_WHISPER_FAST_MODEL.to_string(),
            ModelProfile::Balanced => DEFAULT_WHISPER_BALANCED_MODEL.to_string(),
            ModelProfile::Accurate => DEFAULT_WHISPER_ACCURATE_MODEL.to_string(),
        },
    }
}

fn slice_captured_audio(
    captured: &CapturedAudio,
    start_sample_idx: usize,
    end_sample_idx: usize,
) -> Option<CapturedAudio> {
    let bounded_start = start_sample_idx.min(captured.samples.len());
    let bounded_end = end_sample_idx.min(captured.samples.len());
    if bounded_end <= bounded_start {
        return None;
    }

    Some(CapturedAudio {
        samples: captured.samples[bounded_start..bounded_end].to_vec(),
        sample_rate: captured.sample_rate,
    })
}

fn count_sentence_stoppers(text: &str) -> usize {
    text.chars()
        .filter(|ch| matches!(ch, '.' | '?' | '!'))
        .count()
}

fn tail_after_sentence_count(text: &str, sentence_count: usize) -> String {
    if sentence_count == 0 {
        return text.trim().to_string();
    }

    let mut seen = 0usize;
    for (idx, ch) in text.char_indices() {
        if matches!(ch, '.' | '?' | '!') {
            seen += 1;
            if seen == sentence_count {
                let tail_start = idx + ch.len_utf8();
                return text[tail_start..].trim().to_string();
            }
        }
    }

    String::new()
}

fn append_sentence_commit_text(target: &mut String, segment: &str) {
    let normalized = segment.trim();
    if normalized.is_empty() {
        return;
    }

    if target.is_empty() {
        target.push_str(normalized);
        return;
    }

    target.push(' ');
    target.push_str(normalized);
}

fn merge_committed_with_tail(committed: &str, tail: &str) -> String {
    let normalized_committed = committed.trim();
    let normalized_tail = tail.trim();

    if normalized_committed.is_empty() {
        return normalized_tail.to_string();
    }

    if normalized_tail.is_empty() {
        return normalized_committed.to_string();
    }

    format!("{} {}", normalized_committed, normalized_tail)
}

fn resolve_live_word_profile(fallback_profile: ModelProfile) -> Option<ModelProfile> {
    if is_model_ready(DEFAULT_WHISPER_FAST_MODEL) {
        return Some(ModelProfile::Fast);
    }

    let fallback_model_id = resolve_model_id(
        TranscriptionRuntime::Whisper,
        fallback_profile,
        "",
        MoonshineVariant::Tiny,
    );
    if is_model_ready(&fallback_model_id) {
        return Some(fallback_profile);
    }

    if is_model_ready(DEFAULT_WHISPER_BALANCED_MODEL) {
        return Some(ModelProfile::Balanced);
    }

    if is_model_ready(DEFAULT_WHISPER_ACCURATE_MODEL) {
        return Some(ModelProfile::Accurate);
    }

    None
}

fn resample_for_live_word_vad(samples: &[f32], source_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    if source_rate == LIVE_WORD_VAD_SAMPLE_RATE {
        return samples.to_vec();
    }
    if samples.len() == 1 {
        return vec![samples[0]];
    }

    let target_len = ((samples.len() as u64 * LIVE_WORD_VAD_SAMPLE_RATE as u64)
        / source_rate.max(1) as u64) as usize;
    if target_len == 0 {
        return Vec::new();
    }

    let ratio = source_rate as f64 / LIVE_WORD_VAD_SAMPLE_RATE as f64;
    let mut out = Vec::with_capacity(target_len);
    for idx in 0..target_len {
        let src_pos = (idx as f64) * ratio;
        let left = src_pos.floor() as usize;
        let frac = (src_pos - left as f64) as f32;

        let left_sample = samples[left.min(samples.len() - 1)];
        let right_sample = samples[(left + 1).min(samples.len() - 1)];
        out.push(left_sample + (right_sample - left_sample) * frac);
    }

    out
}

fn map_vad_index_to_source_sample(vad_index: usize, source_rate: u32) -> usize {
    ((vad_index as u64 * source_rate.max(1) as u64) / LIVE_WORD_VAD_SAMPLE_RATE as u64) as usize
}

fn is_likely_live_word_candidate(samples: &[f32]) -> bool {
    if samples.is_empty() {
        return false;
    }

    let mut peak = 0.0_f32;
    let mut power_sum = 0.0_f32;
    for sample in samples {
        let abs = sample.abs();
        if abs > peak {
            peak = abs;
        }
        power_sum += sample * sample;
    }

    let rms = (power_sum / samples.len() as f32).sqrt();
    rms >= LIVE_WORD_MIN_RMS && peak >= LIVE_WORD_MIN_PEAK
}

fn is_low_information_live_word_text(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }

    matches!(
        normalized.as_str(),
        "blank audio" | "[blank_audio]" | "<blank_audio>" | "silence"
    )
}

fn initialize_live_word_vad(app: &AppHandle) -> Result<LiveWordVad, RuntimeError> {
    let _ = app;
    LiveWordVad::new().map_err(map_live_word_vad_init_error)
}

fn map_live_word_vad_init_error(err: LiveWordVadError) -> RuntimeError {
    let code = match err {
        LiveWordVadError::InvalidFrame(_) => "live_word_vad_init_failed",
    };
    runtime_error(
        code,
        format!("Failed to initialize live word VAD: {}", err),
        true,
    )
}

fn append_live_word_text(target: &mut String, candidate: &str) {
    let normalized = candidate.trim();
    if normalized.is_empty() {
        return;
    }

    if target.is_empty() {
        target.push_str(normalized);
        return;
    }

    target.push(' ');
    target.push_str(normalized);
}

fn stop_interim_stream_locked(inner: &mut RuntimeInner) {
    if let Some(stop_tx) = inner.interim_stop_tx.take() {
        let _ = stop_tx.send(());
    }

    inner.interim_session_id = None;
    inner.interim_seq = 0;
    inner.interim_disabled = false;
    inner.live_word_seq = 0;
    inner.live_word_text.clear();
    inner.live_word_processed_samples = 0;
    inner.live_word_vad = None;
}

fn emit_state(app: &AppHandle, state: &RuntimeState) {
    let _ = app.emit("steno://state-changed", state.clone());

    // Keep readiness aggregation aligned with shared helpers.

    let _readiness = readiness::evaluate_readiness(
        map_readiness_permission_from_mic(state.mic_permission),
        map_readiness_permission_from_input_monitoring(state.input_monitoring_permission),
        state.shortcut_ready,
    );

    OverlayRuntimeController::default().sync_overlay_shell(app, map_overlay_phase(&state.phase));
}

fn map_overlay_phase(phase: &Phase) -> OverlayRuntimePhase {
    match phase {
        Phase::Idle => OverlayRuntimePhase::Idle,
        Phase::Recording => OverlayRuntimePhase::Recording,
        Phase::Transcribing => OverlayRuntimePhase::Processing,
        Phase::Error => OverlayRuntimePhase::Error,
    }
}

fn run_action_with_timeout<T, F>(
    timeout_ms: u64,
    action: F,
    action_label: &str,
) -> Result<T, ActionFailure>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let action_name = action_label.to_string();

    thread::spawn(move || {
        let _ = tx.send(action());
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(result) => result.map_err(|message| ActionFailure {
            kind: ActionFailureKind::Failed,
            message,
        }),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ActionFailure {
            kind: ActionFailureKind::Timeout,
            message: format!("{} timed out after {}ms", action_name, timeout_ms),
        }),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ActionFailure {
            kind: ActionFailureKind::Failed,
            message: format!("{} task disconnected unexpectedly", action_name),
        }),
    }
}

fn run_action_with_retry<T, F>(
    retry_count: usize,
    mut action: F,
    action_label: &str,
) -> Result<T, ActionFailure>
where
    F: FnMut() -> Result<T, ActionFailure>,
{
    let mut last_error: Option<ActionFailure> = None;

    for attempt in 0..=retry_count {
        match action() {
            Ok(value) => return Ok(value),
            Err(error) => {
                if attempt < retry_count {
                    log::warn!(
                        "event=output_action_retry action={} attempt={} reason={}",
                        action_label,
                        attempt + 1,
                        error.message
                    );
                }
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or(ActionFailure {
        kind: ActionFailureKind::Failed,
        message: format!("{} failed without a detailed error", action_label),
    }))
}

fn read_frontmost_app_name_with_timeout() -> Result<String, ActionFailure> {
    run_action_with_timeout(
        OUTPUT_ACTION_TIMEOUT_MS,
        read_frontmost_app_name_once,
        "focus_read",
    )
}

fn read_frontmost_app_name_once() -> Result<String, String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to get name of first process whose frontmost is true"#)
        .output()
        .map_err(|e| format!("Failed to execute osascript for focus read: {}", e))?;

    if !output.status.success() {
        let stderr_text = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "focus read osascript exited with status {}: {}",
            output.status, stderr_text
        ));
    }

    let app_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if app_name.is_empty() {
        return Err("focus read returned empty application name".to_string());
    }

    Ok(app_name)
}

fn read_clipboard_text_with_timeout(app: &AppHandle) -> Result<String, ActionFailure> {
    let app_handle = app.clone();
    run_action_with_timeout(
        OUTPUT_ACTION_TIMEOUT_MS,
        move || {
            app_handle
                .clipboard()
                .read_text()
                .map_err(|e| format!("clipboard read failed: {}", e))
        },
        "clipboard_read",
    )
}

fn write_clipboard_text_with_retry(app: &AppHandle, text: &str) -> Result<(), ActionFailure> {
    let app_handle = app.clone();
    let text_value = text.to_string();
    run_action_with_retry(
        OUTPUT_ACTION_RETRY_COUNT,
        move || {
            let app_for_attempt = app_handle.clone();
            let text_for_attempt = text_value.clone();
            run_action_with_timeout(
                OUTPUT_ACTION_TIMEOUT_MS,
                move || {
                    app_for_attempt
                        .clipboard()
                        .write_text(text_for_attempt)
                        .map_err(|e| format!("clipboard write failed: {}", e))
                },
                "clipboard_write",
            )
        },
        "clipboard_write",
    )
}

fn trigger_system_paste_with_retry() -> Result<(), ActionFailure> {
    run_action_with_retry(
        OUTPUT_ACTION_RETRY_COUNT,
        move || {
            run_action_with_timeout(
                OUTPUT_ACTION_TIMEOUT_MS,
                trigger_system_paste_once,
                "system_paste",
            )
        },
        "system_paste",
    )
}

fn trigger_system_paste_once() -> Result<(), String> {
    let status = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to keystroke "v" using command down"#)
        .status()
        .map_err(|e| format!("Failed to execute osascript: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("osascript exited with status: {}", status))
    }
}

fn emit_transcription(app: &AppHandle, result: &TranscriptionResult) {
    let _ = app.emit("steno://transcription-complete", result.clone());
}

fn emit_interim_transcription(app: &AppHandle, frame: &InterimTranscriptionFrame) {
    let _ = app.emit("steno://interim-transcription", frame.clone());
}

fn emit_live_word_output_frame(app: &AppHandle, frame: &LiveWordOutputFrame) {
    let _ = app.emit("steno://live-word-output", frame.clone());
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

pub fn handle_active_hotkey_event(
    app: &AppHandle,
    shortcut_binding: ShortcutBinding,
    is_pressed: bool,
) {
    if let Some(controller) = app.try_state::<RuntimeController>() {
        controller.handle_active_shortcut_event(app, shortcut_binding, is_pressed);
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

    #[test]
    fn default_input_monitoring_permission_is_unknown() {
        let runtime = RuntimeController::new();
        assert_eq!(
            runtime.current_state().input_monitoring_permission,
            InputMonitoringPermission::Unknown
        );
    }

    #[test]
    fn shortcut_bindings_require_unique_values() {
        let result = shortcut::normalize_shortcut_bindings("Fn", "Fn");
        assert!(result.is_err());
        let backend_err = result.expect_err("expected conflict error");
        let err = map_shortcut_error(backend_err);
        assert_eq!(err.code, "shortcut_conflict");
    }

    #[test]
    fn recording_start_timeout_maps_to_timeout_code() {
        let err = map_recording_start_error(
            "Timed out waiting for audio start response after 3000ms".to_string(),
        );
        assert_eq!(err.code, "recording_start_timeout");
    }

    #[test]
    fn run_action_with_timeout_returns_timeout_failure() {
        let result = run_action_with_timeout(
            1,
            || {
                std::thread::sleep(Duration::from_millis(10));
                Ok::<(), String>(())
            },
            "timeout_test",
        );

        assert!(result.is_err());
        let err = result.expect_err("expected timeout failure");
        assert_eq!(err.kind, ActionFailureKind::Timeout);
    }

    #[test]
    fn model_profile_maps_to_whisper_defaults() {
        assert_eq!(
            resolve_model_id(
                TranscriptionRuntime::Whisper,
                ModelProfile::Fast,
                "",
                MoonshineVariant::Tiny
            ),
            DEFAULT_WHISPER_FAST_MODEL
        );
        assert_eq!(
            resolve_model_id(
                TranscriptionRuntime::Whisper,
                ModelProfile::Balanced,
                "",
                MoonshineVariant::Tiny
            ),
            DEFAULT_WHISPER_BALANCED_MODEL
        );
        assert_eq!(
            resolve_model_id(
                TranscriptionRuntime::Whisper,
                ModelProfile::Accurate,
                "",
                MoonshineVariant::Tiny
            ),
            DEFAULT_WHISPER_ACCURATE_MODEL
        );
    }

    #[test]
    fn parakeet_runtime_uses_selected_model() {
        assert_eq!(
            resolve_model_id(
                TranscriptionRuntime::Parakeet,
                ModelProfile::Fast,
                "istupakov/parakeet-tdt-0.6b-v2-onnx",
                MoonshineVariant::Tiny,
            ),
            "istupakov/parakeet-tdt-0.6b-v2-onnx"
        );
    }

    #[test]
    fn parakeet_runtime_falls_back_to_core_default_model() {
        assert_eq!(
            resolve_model_id(
                TranscriptionRuntime::Parakeet,
                ModelProfile::Fast,
                "",
                MoonshineVariant::Tiny
            ),
            DEFAULT_PARAKEET_MODEL
        );
    }

    #[test]
    fn runtime_defaults_include_parakeet_model_id() {
        let runtime = RuntimeController::new();
        assert_eq!(
            runtime.current_state().parakeet_model_id,
            DEFAULT_PARAKEET_ONNX_MODEL
        );
    }

    #[test]
    fn live_word_vad_invalid_input_maps_to_init_failed_code() {
        let err = map_live_word_vad_init_error(LiveWordVadError::InvalidFrame(
            "invalid frame".to_string(),
        ));
        assert_eq!(err.code, "live_word_vad_init_failed");
    }

    #[test]
    fn live_word_vad_audio_processing_maps_to_init_failed_code() {
        let err = map_live_word_vad_init_error(LiveWordVadError::InvalidFrame(
            "runtime failure".to_string(),
        ));
        assert_eq!(err.code, "live_word_vad_init_failed");
    }

    #[test]
    fn live_word_resampler_noop_for_16khz() {
        let source = vec![0.1_f32, -0.2, 0.3, -0.4];
        let out = resample_for_live_word_vad(&source, 16_000);
        assert_eq!(out, source);
    }

    #[test]
    fn live_word_index_mapping_scales_by_sample_rate() {
        assert_eq!(map_vad_index_to_source_sample(16_000, 48_000), 48_000);
        assert_eq!(map_vad_index_to_source_sample(8_000, 8_000), 4_000);
    }

    #[test]
    fn low_information_live_word_text_is_filtered() {
        assert!(is_low_information_live_word_text("blank audio"));
        assert!(is_low_information_live_word_text(" [blank_audio] "));
        assert!(!is_low_information_live_word_text("hello world"));
    }
}
