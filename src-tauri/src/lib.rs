mod audio_capture;
mod post_process;
mod runtime;
mod shortcut;

use runtime::{
    RecordMode, RuntimeController, RuntimeInitResult, RuntimeState, TranscriptionResult,
};
use tauri::{Emitter, Manager};
use tauri_plugin_log::{Builder as LogBuilder, RotationStrategy, Target, TargetKind};

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
fn set_record_mode(
    app: tauri::AppHandle,
    mode: RecordMode,
    runtime: tauri::State<'_, RuntimeController>,
) -> Result<(), String> {
    runtime.set_mode(&app, mode);
    Ok(())
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
        .invoke_handler(tauri::generate_handler![
            initialize_runtime,
            set_mic_permission,
            set_record_mode,
            get_record_mode,
            start_recording_manual,
            stop_recording_manual,
            get_runtime_state,
        ])
        .setup(|app| {
            let app_handle = app.handle();

            if let Some(main_window) = app_handle.get_webview_window("main") {
                if let Err(e) = main_window.show() {
                    log::error!("event=window_show_failed error={}", e);
                }
                if let Err(e) = main_window.set_focus() {
                    log::error!("event=window_focus_failed error={}", e);
                }
            } else {
                log::error!("event=window_missing label=main");
            }

            if let Some(runtime) = app_handle.try_state::<RuntimeController>() {
                let state = runtime.current_state();
                let _ = app_handle.emit("steno://state-changed", state);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running steno application");
}
