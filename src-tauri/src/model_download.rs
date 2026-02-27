use hf_hub::api::tokio::{Api, Progress};
use hf_hub::{Cache, Repo, RepoType};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter};
use transcriber_core::transcriber::DEFAULT_PARAKEET_MODEL;

const MODEL_DOWNLOAD_EVENT: &str = "steno://model-download-state-changed";
const MODEL_REVISION: &str = "main";
const PROGRESS_EMIT_THROTTLE_MS: u64 = 200;
const USEFUL_SENSORS_MOONSHINE_REPO: &str = "UsefulSensors/moonshine";
const PARAKEET_DEFAULT_ONNX_REPO: &str = "istupakov/parakeet-tdt-0.6b-v3-onnx";
const REQUIRED_WHISPER_MODEL_FILES: [&str; 3] =
    ["config.json", "tokenizer.json", "model.safetensors"];
const REQUIRED_PARAKEET_MODEL_FILES: [&str; 4] = [
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
];

struct ModelCatalogSpec {
    key: &'static str,
    runtime: &'static str,
    profile: &'static str,
    model_id: &'static str,
    repo_id: &'static str,
    required_files: &'static [&'static str],
}

// Keep model catalog centralized.

const MODEL_CATALOG: [ModelCatalogSpec; 7] = [
    ModelCatalogSpec {
        key: "whisper-fast",
        runtime: "whisper",
        profile: "fast",
        model_id: "openai/whisper-tiny",
        repo_id: "openai/whisper-tiny",
        required_files: &REQUIRED_WHISPER_MODEL_FILES,
    },
    ModelCatalogSpec {
        key: "whisper-balanced",
        runtime: "whisper",
        profile: "balanced",
        model_id: "openai/whisper-base",
        repo_id: "openai/whisper-base",
        required_files: &REQUIRED_WHISPER_MODEL_FILES,
    },
    ModelCatalogSpec {
        key: "whisper-accurate",
        runtime: "whisper",
        profile: "accurate",
        model_id: "openai/whisper-small",
        repo_id: "openai/whisper-small",
        required_files: &REQUIRED_WHISPER_MODEL_FILES,
    },
    ModelCatalogSpec {
        key: "parakeet-v3",
        runtime: "parakeet",
        profile: "v3",
        model_id: PARAKEET_DEFAULT_ONNX_REPO,
        repo_id: PARAKEET_DEFAULT_ONNX_REPO,
        required_files: &REQUIRED_PARAKEET_MODEL_FILES,
    },
    ModelCatalogSpec {
        key: "parakeet-v2",
        runtime: "parakeet",
        profile: "v2",
        model_id: "istupakov/parakeet-tdt-0.6b-v2-onnx",
        repo_id: "istupakov/parakeet-tdt-0.6b-v2-onnx",
        required_files: &REQUIRED_PARAKEET_MODEL_FILES,
    },
    ModelCatalogSpec {
        key: "moonshine-tiny",
        runtime: "moonshine",
        profile: "tiny",
        model_id: "moonshine-tiny",
        repo_id: USEFUL_SENSORS_MOONSHINE_REPO,
        required_files: &[
            "onnx/merged/tiny/float/encoder_model.onnx",
            "onnx/merged/tiny/float/decoder_model_merged.onnx",
            "ctranslate2/tiny/tokenizer.json",
        ],
    },
    ModelCatalogSpec {
        key: "moonshine-base",
        runtime: "moonshine",
        profile: "base",
        model_id: "moonshine-base",
        repo_id: USEFUL_SENSORS_MOONSHINE_REPO,
        required_files: &[
            "onnx/merged/base/float/encoder_model.onnx",
            "onnx/merged/base/float/decoder_model_merged.onnx",
            "ctranslate2/base/tokenizer.json",
        ],
    },
];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadStatus {
    NotDownloaded,
    Queued,
    Downloading,
    Ready,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCatalogEntry {
    pub key: String,
    pub runtime: String,
    pub profile: String,
    pub model_id: String,
    pub repo_id: String,
    pub required_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadEntry {
    #[serde(flatten)]
    pub catalog: ModelCatalogEntry,
    pub status: ModelDownloadStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub last_error: Option<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadsSnapshot {
    pub models: Vec<ModelDownloadEntry>,
    pub queue: Vec<String>,
    pub active_model_key: Option<String>,
}

struct ModelDownloadState {
    models: BTreeMap<String, ModelDownloadEntry>,
    queue: VecDeque<String>,
    active_model_key: Option<String>,
    active_handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct ModelDownloadManager {
    inner: Arc<Mutex<ModelDownloadState>>,
}

impl ModelDownloadManager {
    pub fn new() -> Self {
        let mut models = BTreeMap::new();
        for spec in MODEL_CATALOG.iter() {
            let is_ready = is_catalog_entry_ready(spec);
            let entry = ModelDownloadEntry {
                catalog: ModelCatalogEntry {
                    key: spec.key.to_string(),
                    runtime: spec.runtime.to_string(),
                    profile: spec.profile.to_string(),
                    model_id: spec.model_id.to_string(),
                    repo_id: spec.repo_id.to_string(),
                    required_files: spec
                        .required_files
                        .iter()
                        .map(|file| file.to_string())
                        .collect(),
                },
                status: if is_ready {
                    ModelDownloadStatus::Ready
                } else {
                    ModelDownloadStatus::NotDownloaded
                },
                downloaded_bytes: 0,
                total_bytes: 0,
                speed_bytes_per_sec: 0,
                last_error: None,
                updated_at_ms: current_timestamp_ms(),
            };

            models.insert(spec.key.to_string(), entry);
        }

        Self {
            inner: Arc::new(Mutex::new(ModelDownloadState {
                models,
                queue: VecDeque::new(),
                active_model_key: None,
                active_handle: None,
            })),
        }
    }

    pub fn emit_state(&self, app: &AppHandle) {
        let _ = app.emit(MODEL_DOWNLOAD_EVENT, self.snapshot());
    }

    pub fn list_model_downloads(&self) -> ModelDownloadsSnapshot {
        self.refresh_ready_states();
        self.snapshot()
    }

    pub fn start_model_download(&self, app: &AppHandle, model_key: &str) -> Result<(), String> {
        self.refresh_ready_states();

        let mut should_start_next = false;

        {
            let mut state = self.inner.lock().expect("model download mutex poisoned");
            let status = state
                .models
                .get(model_key)
                .map(|entry| entry.status)
                .ok_or_else(|| format!("Unknown model key '{}'", model_key))?;

            match status {
                ModelDownloadStatus::Ready
                | ModelDownloadStatus::Queued
                | ModelDownloadStatus::Downloading => {}
                ModelDownloadStatus::NotDownloaded
                | ModelDownloadStatus::Failed
                | ModelDownloadStatus::Canceled => {
                    {
                        let entry = state
                            .models
                            .get_mut(model_key)
                            .expect("model should exist while queueing");
                        entry.status = ModelDownloadStatus::Queued;
                        entry.downloaded_bytes = 0;
                        entry.total_bytes = 0;
                        entry.speed_bytes_per_sec = 0;
                        entry.last_error = None;
                        entry.updated_at_ms = current_timestamp_ms();
                    }

                    if !state.queue.iter().any(|queued| queued == model_key) {
                        state.queue.push_back(model_key.to_string());
                    }
                }
            }

            if state.active_model_key.is_none() {
                should_start_next = true;
            }
        }

        self.emit_state(app);
        if should_start_next {
            self.start_next_download(app);
        }
        Ok(())
    }

    pub fn cancel_model_download(&self, app: &AppHandle, model_key: &str) -> Result<(), String> {
        let mut should_start_next = false;

        {
            let mut state = self.inner.lock().expect("model download mutex poisoned");
            if !state.models.contains_key(model_key) {
                return Err(format!("Unknown model key '{}'", model_key));
            }

            let is_active = state.active_model_key.as_deref() == Some(model_key);
            let removed_from_queue = if !is_active {
                remove_from_queue(&mut state.queue, model_key)
            } else {
                false
            };

            if is_active {
                if let Some(active_handle) = state.active_handle.take() {
                    active_handle.abort();
                }
                state.active_model_key = None;
                should_start_next = true;
            }

            if is_active || removed_from_queue {
                let entry = state
                    .models
                    .get_mut(model_key)
                    .expect("model should exist while canceling");
                entry.status = ModelDownloadStatus::Canceled;
                entry.speed_bytes_per_sec = 0;
                entry.last_error = None;
                entry.updated_at_ms = current_timestamp_ms();
            }
        }

        self.emit_state(app);
        if should_start_next {
            self.start_next_download(app);
        }
        Ok(())
    }

    fn snapshot(&self) -> ModelDownloadsSnapshot {
        let state = self.inner.lock().expect("model download mutex poisoned");
        let models = MODEL_CATALOG
            .iter()
            .filter_map(|spec| state.models.get(spec.key).cloned())
            .collect::<Vec<_>>();

        ModelDownloadsSnapshot {
            models,
            queue: state.queue.iter().cloned().collect(),
            active_model_key: state.active_model_key.clone(),
        }
    }

    fn refresh_ready_states(&self) {
        let mut state = self.inner.lock().expect("model download mutex poisoned");
        for spec in MODEL_CATALOG.iter() {
            let Some(entry) = state.models.get_mut(spec.key) else {
                continue;
            };

            if matches!(
                entry.status,
                ModelDownloadStatus::Queued | ModelDownloadStatus::Downloading
            ) {
                continue;
            }

            let is_ready = is_catalog_entry_ready(spec);
            if is_ready {
                entry.status = ModelDownloadStatus::Ready;
                entry.last_error = None;
                entry.updated_at_ms = current_timestamp_ms();
            } else if matches!(entry.status, ModelDownloadStatus::Ready) {
                entry.status = ModelDownloadStatus::NotDownloaded;
                entry.downloaded_bytes = 0;
                entry.total_bytes = 0;
                entry.speed_bytes_per_sec = 0;
                entry.updated_at_ms = current_timestamp_ms();
            }
        }
    }

    fn start_next_download(&self, app: &AppHandle) {
        let next_job = {
            let mut state = self.inner.lock().expect("model download mutex poisoned");
            if state.active_model_key.is_some() {
                return;
            }

            let mut selected_job: Option<(String, String, String, Vec<String>)> = None;
            while let Some(next_model_key) = state.queue.pop_front() {
                let Some(entry) = state.models.get_mut(&next_model_key) else {
                    continue;
                };

                entry.status = ModelDownloadStatus::Downloading;
                entry.downloaded_bytes = 0;
                entry.total_bytes = 0;
                entry.speed_bytes_per_sec = 0;
                entry.last_error = None;
                entry.updated_at_ms = current_timestamp_ms();

                let model_id = entry.catalog.model_id.clone();
                let repo_id = entry.catalog.repo_id.clone();
                let required_files = entry.catalog.required_files.clone();

                state.active_model_key = Some(next_model_key.clone());
                selected_job = Some((next_model_key, model_id, repo_id, required_files));
                break;
            }
            selected_job
        };

        let Some((model_key, _model_id, repo_id, required_files)) = next_job else {
            self.emit_state(app);
            return;
        };

        self.emit_state(app);

        let manager = self.clone();
        let app_handle = app.clone();
        let key_for_task = model_key.clone();
        let task_handle = tauri::async_runtime::spawn(async move {
            manager
                .run_download_job(app_handle, key_for_task, repo_id, required_files)
                .await;
        });

        let mut state = self.inner.lock().expect("model download mutex poisoned");
        if state.active_model_key.as_deref() == Some(model_key.as_str()) {
            state.active_handle = Some(task_handle);
        } else {
            task_handle.abort();
        }
    }

    async fn run_download_job(
        &self,
        app: AppHandle,
        model_key: String,
        repo_id: String,
        required_files: Vec<String>,
    ) {
        let result = self
            .download_required_files(&app, &model_key, &repo_id, &required_files)
            .await;
        self.finish_download(&app, &model_key, result);
    }

    async fn download_required_files(
        &self,
        app: &AppHandle,
        model_key: &str,
        repo_id: &str,
        required_files: &[String],
    ) -> Result<(), String> {
        let api = Api::new().map_err(|err| format!("Model downloader setup failed: {}", err))?;
        let repo = api.repo(Repo::with_revision(
            repo_id.to_string(),
            RepoType::Model,
            MODEL_REVISION.to_string(),
        ));
        let cache_repo = cache_repo_for_model(repo_id);

        let mut completed_bytes = 0_u64;
        let mut known_total_bytes = 0_u64;

        for file_name in required_files {
            if let Some(cached_path) = cache_repo.get(file_name) {
                let cached_size = read_file_size(&cached_path);
                completed_bytes = completed_bytes.saturating_add(cached_size);
                known_total_bytes = known_total_bytes.saturating_add(cached_size);
                self.apply_progress_update(app, model_key, completed_bytes, known_total_bytes, 0);
                continue;
            }

            let progress_reporter = DownloadProgressReporter::new(
                self.clone(),
                app.clone(),
                model_key.to_string(),
                completed_bytes,
                known_total_bytes,
            );

            let downloaded_path = repo
                .download_with_progress(file_name, progress_reporter)
                .await
                .map_err(|err| format!("Failed downloading {}: {}", file_name, err))?;

            let downloaded_size = read_file_size(&downloaded_path);
            completed_bytes = completed_bytes.saturating_add(downloaded_size);
            known_total_bytes = known_total_bytes.saturating_add(downloaded_size);
            self.apply_progress_update(app, model_key, completed_bytes, known_total_bytes, 0);
        }

        Ok(())
    }

    fn finish_download(&self, app: &AppHandle, model_key: &str, result: Result<(), String>) {
        let mut should_start_next = false;

        {
            let mut state = self.inner.lock().expect("model download mutex poisoned");

            if state.active_model_key.as_deref() == Some(model_key) {
                state.active_model_key = None;
                state.active_handle = None;
                should_start_next = true;
            }

            let Some(entry) = state.models.get_mut(model_key) else {
                drop(state);
                self.emit_state(app);
                if should_start_next {
                    self.start_next_download(app);
                }
                return;
            };

            if entry.status == ModelDownloadStatus::Canceled {
                entry.speed_bytes_per_sec = 0;
                entry.updated_at_ms = current_timestamp_ms();
            } else {
                match result {
                    Ok(()) => {
                        if is_model_ready(&entry.catalog.model_id) {
                            entry.status = ModelDownloadStatus::Ready;
                            entry.last_error = None;
                            if entry.total_bytes == 0 {
                                entry.total_bytes = total_cached_size(
                                    &entry.catalog.repo_id,
                                    &entry.catalog.required_files,
                                );
                            }
                            entry.downloaded_bytes = entry.total_bytes;
                            entry.speed_bytes_per_sec = 0;
                        } else {
                            entry.status = ModelDownloadStatus::Failed;
                            entry.last_error = Some(
                                "Download finished but model artifacts were not found in cache."
                                    .to_string(),
                            );
                            entry.speed_bytes_per_sec = 0;
                        }
                        entry.updated_at_ms = current_timestamp_ms();
                    }
                    Err(message) => {
                        entry.status = ModelDownloadStatus::Failed;
                        entry.last_error = Some(message);
                        entry.speed_bytes_per_sec = 0;
                        entry.updated_at_ms = current_timestamp_ms();
                    }
                }
            }
        }

        self.emit_state(app);
        if should_start_next {
            self.start_next_download(app);
        }
    }

    fn apply_progress_update(
        &self,
        app: &AppHandle,
        model_key: &str,
        downloaded_bytes: u64,
        total_bytes: u64,
        speed_bytes_per_sec: u64,
    ) {
        {
            let mut state = self.inner.lock().expect("model download mutex poisoned");
            let Some(entry) = state.models.get_mut(model_key) else {
                return;
            };

            if entry.status != ModelDownloadStatus::Downloading {
                return;
            }

            entry.downloaded_bytes = downloaded_bytes;
            entry.total_bytes = total_bytes;
            entry.speed_bytes_per_sec = speed_bytes_per_sec;
            entry.updated_at_ms = current_timestamp_ms();
        }

        self.emit_state(app);
    }
}

struct ProgressEmitterState {
    file_total_bytes: AtomicU64,
    file_downloaded_bytes: AtomicU64,
    started_at_ms: AtomicU64,
    last_emit_ms: AtomicU64,
}

#[derive(Clone)]
struct DownloadProgressReporter {
    manager: ModelDownloadManager,
    app: AppHandle,
    model_key: String,
    completed_before_file: u64,
    known_total_before_file: u64,
    shared: Arc<ProgressEmitterState>,
}

impl DownloadProgressReporter {
    fn new(
        manager: ModelDownloadManager,
        app: AppHandle,
        model_key: String,
        completed_before_file: u64,
        known_total_before_file: u64,
    ) -> Self {
        Self {
            manager,
            app,
            model_key,
            completed_before_file,
            known_total_before_file,
            shared: Arc::new(ProgressEmitterState {
                file_total_bytes: AtomicU64::new(0),
                file_downloaded_bytes: AtomicU64::new(0),
                started_at_ms: AtomicU64::new(0),
                last_emit_ms: AtomicU64::new(0),
            }),
        }
    }

    fn emit_progress(&self, force: bool) {
        let now_ms = current_timestamp_ms();
        if !force {
            let previous_emit = self.shared.last_emit_ms.load(Ordering::Relaxed);
            if now_ms.saturating_sub(previous_emit) < PROGRESS_EMIT_THROTTLE_MS {
                return;
            }
        }

        self.shared.last_emit_ms.store(now_ms, Ordering::Relaxed);

        let downloaded_in_file = self.shared.file_downloaded_bytes.load(Ordering::Relaxed);
        let file_total = self.shared.file_total_bytes.load(Ordering::Relaxed);
        let total_downloaded = self
            .completed_before_file
            .saturating_add(downloaded_in_file);
        let total_known = self.known_total_before_file.saturating_add(file_total);
        let started_at_ms = self.shared.started_at_ms.load(Ordering::Relaxed);
        let elapsed_ms = now_ms.saturating_sub(started_at_ms).max(1);
        let speed_bytes_per_sec =
            ((downloaded_in_file as u128 * 1000_u128) / elapsed_ms as u128) as u64;

        self.manager.apply_progress_update(
            &self.app,
            &self.model_key,
            total_downloaded,
            total_known,
            speed_bytes_per_sec,
        );
    }
}

impl Progress for DownloadProgressReporter {
    async fn init(&mut self, size: usize, _filename: &str) {
        self.shared
            .file_total_bytes
            .store(size as u64, Ordering::Relaxed);
        self.shared
            .file_downloaded_bytes
            .store(0, Ordering::Relaxed);
        self.shared
            .started_at_ms
            .store(current_timestamp_ms(), Ordering::Relaxed);
        self.emit_progress(true);
    }

    async fn update(&mut self, size: usize) {
        self.shared
            .file_downloaded_bytes
            .fetch_add(size as u64, Ordering::Relaxed);
        self.emit_progress(false);
    }

    async fn finish(&mut self) {
        let total_bytes = self.shared.file_total_bytes.load(Ordering::Relaxed);
        self.shared
            .file_downloaded_bytes
            .store(total_bytes, Ordering::Relaxed);
        self.emit_progress(true);
    }
}

fn remove_from_queue(queue: &mut VecDeque<String>, model_key: &str) -> bool {
    if let Some(position) = queue.iter().position(|queued| queued == model_key) {
        queue.remove(position);
        true
    } else {
        false
    }
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn canonical_model_id(model_id: &str) -> &str {
    if model_id.eq_ignore_ascii_case(DEFAULT_PARAKEET_MODEL) {
        PARAKEET_DEFAULT_ONNX_REPO
    } else {
        model_id
    }
}

fn cache_repo_for_model(model_id: &str) -> hf_hub::CacheRepo {
    let repo_id = canonical_model_id(model_id);
    Cache::default().repo(Repo::with_revision(
        repo_id.to_string(),
        RepoType::Model,
        MODEL_REVISION.to_string(),
    ))
}

fn read_file_size(path: &Path) -> u64 {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn total_cached_size(model_id: &str, required_files: &[String]) -> u64 {
    let cache_repo = cache_repo_for_model(model_id);
    required_files
        .iter()
        .filter_map(|file_name| cache_repo.get(file_name))
        .map(|path| read_file_size(&path))
        .sum()
}

fn model_spec_for_id(model_id: &str) -> Option<&'static ModelCatalogSpec> {
    let resolved = canonical_model_id(model_id);
    MODEL_CATALOG
        .iter()
        .find(|spec| canonical_model_id(spec.model_id).eq_ignore_ascii_case(resolved))
}

fn is_catalog_entry_ready(spec: &ModelCatalogSpec) -> bool {
    let cache_repo = cache_repo_for_model(spec.repo_id);
    spec.required_files
        .iter()
        .all(|file_name| cache_repo.get(file_name).is_some())
}

pub fn is_model_ready(model_id: &str) -> bool {
    let Some(spec) = model_spec_for_id(model_id) else {
        return false;
    };

    is_catalog_entry_ready(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_whisper_and_parakeet_entries_and_moonshine() {
        assert_eq!(MODEL_CATALOG.len(), 7);
        assert_eq!(MODEL_CATALOG[0].profile, "fast");
        assert_eq!(MODEL_CATALOG[1].profile, "balanced");
        assert_eq!(MODEL_CATALOG[2].profile, "accurate");
        assert_eq!(MODEL_CATALOG[3].runtime, "parakeet");
        assert_eq!(MODEL_CATALOG[4].runtime, "parakeet");
        assert_eq!(MODEL_CATALOG[5].runtime, "moonshine");
        assert_eq!(MODEL_CATALOG[6].runtime, "moonshine");
    }

    #[test]
    fn default_parakeet_model_maps_to_onnx_catalog_entry() {
        let spec = model_spec_for_id(DEFAULT_PARAKEET_MODEL).expect("expected parakeet catalog");
        assert_eq!(spec.model_id, PARAKEET_DEFAULT_ONNX_REPO);
    }

    #[test]
    fn ready_check_for_unknown_model_returns_false() {
        assert!(!is_model_ready("openai/unknown-model"));
    }
}
