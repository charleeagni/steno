use handy_keys::{Hotkey, HotkeyManager, HotkeyState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tauri::AppHandle;

pub struct FnShortcutManager {
    running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
}

impl FnShortcutManager {
    pub fn start(app: AppHandle) -> Result<Self, String> {
        let manager = HotkeyManager::new()
            .map_err(|e| format!("Failed to create hotkey manager for Fn key: {}", e))?;

        let hotkey = "fn"
            .parse::<Hotkey>()
            .map_err(|e| format!("Failed to parse Fn hotkey: {}", e))?;

        let hotkey_id = manager
            .register(hotkey)
            .map_err(|e| format!("Failed to register Fn hotkey: {}", e))?;

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let thread_handle = thread::spawn(move || {
            while running_clone.load(Ordering::SeqCst) {
                while let Some(event) = manager.try_recv() {
                    if event.id != hotkey_id {
                        continue;
                    }

                    let is_pressed = event.state == HotkeyState::Pressed;
                    crate::runtime::handle_fn_hotkey_event(&app, is_pressed);
                }

                thread::sleep(std::time::Duration::from_millis(8));
            }

            let _ = manager.unregister(hotkey_id);
        });

        Ok(Self {
            running,
            thread_handle: Some(thread_handle),
        })
    }
}

impl Drop for FnShortcutManager {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
