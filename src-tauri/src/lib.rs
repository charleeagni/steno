mod audio_capture;
mod model_download;
mod post_process;
mod runtime;
mod shortcut;

use model_download::{ModelDownloadManager, ModelDownloadsSnapshot};
use runtime::{
    ClipboardPolicy, ModelProfile, RecordMode, RuntimeController, RuntimeInitResult, RuntimeState,
    TranscriptionResult, TranscriptionRuntime,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::utils::config::Color;
use tauri::{Emitter, Manager};
use tauri_plugin_log::{Builder as LogBuilder, RotationStrategy, Target, TargetKind};

const TRAY_MENU_SHOW: &str = "tray_show";
const TRAY_MENU_HIDE: &str = "tray_hide";
const TRAY_MENU_QUIT: &str = "tray_quit";

struct AppLifecycleState {
    is_quitting: AtomicBool,
}

impl AppLifecycleState {
    fn new() -> Self {
        Self {
            is_quitting: AtomicBool::new(false),
        }
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(main_window) = app.get_webview_window("main") {
        if let Err(e) = main_window.show() {
            log::error!("event=window_show_failed error={}", e);
        }
        if let Err(e) = main_window.set_focus() {
            log::error!("event=window_focus_failed error={}", e);
        }
    } else {
        log::error!("event=window_missing label=main");
    }
}

fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(main_window) = app.get_webview_window("main") {
        if let Err(e) = main_window.hide() {
            log::error!("event=window_hide_failed error={}", e);
        }
    } else {
        log::error!("event=window_missing label=main");
    }
}

fn hide_overlay_window(app: &tauri::AppHandle) {
    if let Some(overlay_window) = app.get_webview_window("overlay") {
        let _ = overlay_window.set_shadow(false);
        let _ = overlay_window.set_background_color(Some(Color(0, 0, 0, 0)));
        let _ = overlay_window.eval(
            "document.documentElement.style.background='transparent';document.body.style.background='transparent';",
        );
        if let Err(e) = overlay_window.hide() {
            log::error!("event=window_hide_failed label=overlay error={}", e);
        }
    } else {
        log::error!("event=window_missing label=overlay");
    }
}

fn handle_tray_menu_event(app: &tauri::AppHandle, menu_id: &str) {
    match menu_id {
        TRAY_MENU_SHOW => show_main_window(app),
        TRAY_MENU_HIDE => hide_main_window(app),
        TRAY_MENU_QUIT => {
            if let Some(lifecycle_state) = app.try_state::<AppLifecycleState>() {
                lifecycle_state.is_quitting.store(true, Ordering::SeqCst);
            }
            app.exit(0);
        }
        _ => {}
    }
}

#[tauri::command]
fn initialize_runtime(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<RuntimeInitResult, String> {
    Ok(runtime.initialize(&app))
}

#[tauri::command]
fn set_mic_permission(
    app: tauri::AppHandle,
    granted: bool,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime.set_mic_permission(&app, granted);
    Ok(())
}

#[tauri::command]
fn set_input_monitoring_permission(
    app: tauri::AppHandle,
    granted: bool,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime
        .set_input_monitoring_permission(&app, granted)
        .map_err(|err| {
            runtime.publish_error(&app, err.clone());
            err.message
        })
}

#[tauri::command]
fn set_record_mode(
    app: tauri::AppHandle,
    mode: RecordMode,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime.set_mode(&app, mode).map_err(|err| {
        runtime.publish_error(&app, err.clone());
        err.message
    })
}

#[tauri::command]
fn set_clipboard_policy(
    app: tauri::AppHandle,
    policy: ClipboardPolicy,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime.set_clipboard_policy(&app, policy).map_err(|err| {
        runtime.publish_error(&app, err.clone());
        err.message
    })
}

#[tauri::command]
fn set_hotkey_bindings(
    app: tauri::AppHandle,
    push: String,
    toggle: String,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime
        .set_hotkey_bindings(&app, push, toggle)
        .map_err(|err| {
            runtime.publish_error(&app, err.clone());
            err.message
        })
}

#[tauri::command]
async fn capture_hotkey(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<Option<String>, String> {
    let runtime_controller = runtime.inner().clone();
    let should_resume = runtime_controller.suspend_shortcuts_for_capture();
    let capture_result = tauri::async_runtime::spawn_blocking(move || {
        shortcut::capture_hotkey(Duration::from_secs(20))
    })
    .await
    .map_err(|error| format!("Shortcut capture task failed: {}", error))?;

    if let Err(err) = runtime_controller.resume_shortcuts_after_capture(&app, should_resume) {
        runtime_controller.publish_error(&app, err.clone());
        return Err(err.message);
    }

    capture_result
}

#[tauri::command]
fn set_runtime_selection(
    app: tauri::AppHandle,
    selection: TranscriptionRuntime,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime
        .set_runtime_selection(&app, selection)
        .map_err(|err| err.message)
}

#[tauri::command]
fn set_model_profile(
    app: tauri::AppHandle,
    profile: ModelProfile,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime
        .set_model_profile(&app, profile)
        .map_err(|err| err.message)
}

#[tauri::command]
fn set_parakeet_model_id(
    app: tauri::AppHandle,
    model_id: String,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime
        .set_parakeet_model_id(&app, model_id)
        .map_err(|err| err.message)
}

#[tauri::command]
fn list_model_downloads(
    model_downloads: tauri::State<'_, ModelDownloadManager>,
) -> Result<ModelDownloadsSnapshot, String> {
    Ok(model_downloads.list_model_downloads())
}

#[tauri::command]
fn start_model_download(
    app: tauri::AppHandle,
    model_key: String,
    model_downloads: tauri::State<'_, ModelDownloadManager>,
) -> Result<(), String> {
    model_downloads.start_model_download(&app, &model_key)
}

#[tauri::command]
fn cancel_model_download(
    app: tauri::AppHandle,
    model_key: String,
    model_downloads: tauri::State<'_, ModelDownloadManager>,
) -> Result<(), String> {
    model_downloads.cancel_model_download(&app, &model_key)
}

#[tauri::command]
fn get_record_mode(runtime: tauri::State<'_, RuntimeController>) -> Result<RecordMode, String> {
    Ok(runtime.mode())
}

#[tauri::command]
fn get_runtime_state(runtime: tauri::State<'_, RuntimeController>) -> Result<RuntimeState, String> {
    Ok(runtime.current_state())
}

#[tauri::command]
fn start_recording_manual(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    match runtime.start_recording(&app, "manual") {
        Ok(()) => Ok(()),
        Err(err) => {
            runtime.publish_error(&app, err.clone());
            Err(err.message)
        }
    }
}

#[tauri::command]
async fn stop_recording_manual(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<TranscriptionResult, String> {
    match runtime.stop_recording_and_transcribe(&app, "manual").await {
        Ok(result) => Ok(result),
        Err(err) => {
            runtime.publish_error(&app, err.clone());
            Err(err.message)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            LogBuilder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(750_000)
                .rotation_strategy(RotationStrategy::KeepAll)
                .clear_targets()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir {
                        file_name: Some("steno".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_macos_permissions::init())
        .manage(RuntimeController::new())
        .manage(ModelDownloadManager::new())
        .manage(AppLifecycleState::new())
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let should_allow_close = window
                    .app_handle()
                    .try_state::<AppLifecycleState>()
                    .map(|state| state.is_quitting.load(Ordering::SeqCst))
                    .unwrap_or(false);

                if !should_allow_close {
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        log::error!(
                            "event=window_hide_failed source=close_requested error={}",
                            e
                        );
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            initialize_runtime,
            set_mic_permission,
            set_input_monitoring_permission,
            set_record_mode,
            set_clipboard_policy,
            set_hotkey_bindings,
            capture_hotkey,
            set_runtime_selection,
            set_model_profile,
            set_parakeet_model_id,
            list_model_downloads,
            start_model_download,
            cancel_model_download,
            get_record_mode,
            start_recording_manual,
            stop_recording_manual,
            get_runtime_state,
        ])
        .setup(|app| {
            let app_handle = app.handle();
            let tray_menu = tauri::menu::MenuBuilder::new(app_handle)
                .text(TRAY_MENU_SHOW, "Show Steno")
                .text(TRAY_MENU_HIDE, "Hide Steno")
                .separator()
                .text(TRAY_MENU_QUIT, "Quit Steno")
                .build()?;

            let mut tray_builder = tauri::tray::TrayIconBuilder::with_id("steno-tray")
                .menu(&tray_menu)
                .tooltip("Steno")
                .icon_as_template(true)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    handle_tray_menu_event(app, event.id().as_ref());
                });

            if let Some(icon) = app_handle.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }

            if let Err(e) = tray_builder.build(app_handle) {
                log::error!("event=tray_init_failed error={}", e);
            }

            show_main_window(&app_handle);
            hide_overlay_window(&app_handle);

            if let Some(runtime) = app_handle.try_state::<RuntimeController>() {
                let state = runtime.current_state();
                let _ = app_handle.emit("steno://state-changed", state);
            }

            if let Some(model_downloads) = app_handle.try_state::<ModelDownloadManager>() {
                model_downloads.emit_state(&app_handle);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running steno application");
}
