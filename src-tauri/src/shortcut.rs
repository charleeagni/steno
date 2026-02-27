use handy_keys::{Hotkey, HotkeyId, HotkeyManager, HotkeyState, Key, KeyboardListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutBinding {
    PushToTalk,
    Toggle,
}

pub struct FnShortcutManager {
    running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
}

impl FnShortcutManager {
    pub fn start(
        app: AppHandle,
        shortcut_bindings: &[(String, ShortcutBinding)],
    ) -> Result<Self, String> {
        if shortcut_bindings.is_empty() {
            return Err("At least one shortcut binding is required.".to_string());
        }

        let manager =
            HotkeyManager::new().map_err(|e| format!("Failed to create hotkey manager: {}", e))?;

        let mut registered_hotkeys: Vec<(HotkeyId, ShortcutBinding)> = Vec::new();

        for (shortcut, binding) in shortcut_bindings {
            let hotkey = shortcut
                .parse::<Hotkey>()
                .map_err(|e| format!("Failed to parse shortcut '{}': {}", shortcut, e))?;

            let hotkey_id = manager
                .register(hotkey)
                .map_err(|e| format!("Failed to register shortcut '{}': {}", shortcut, e))?;

            registered_hotkeys.push((hotkey_id, *binding));
        }

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let thread_handle = thread::spawn(move || {
            while running_clone.load(Ordering::SeqCst) {
                while let Some(event) = manager.try_recv() {
                    let Some((_, binding)) = registered_hotkeys
                        .iter()
                        .find(|(hotkey_id, _)| *hotkey_id == event.id)
                    else {
                        continue;
                    };

                    let is_pressed = event.state == HotkeyState::Pressed;
                    crate::runtime::handle_active_hotkey_event(&app, *binding, is_pressed);
                }

                thread::sleep(std::time::Duration::from_millis(8));
            }

            for (hotkey_id, _) in registered_hotkeys {
                let _ = manager.unregister(hotkey_id);
            }
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

pub fn capture_hotkey(timeout: Duration) -> Result<Option<String>, String> {
    let listener = KeyboardListener::new()
        .map_err(|e| format!("Failed to start keyboard capture listener: {}", e))?;
    let mut latest_hotkey: Option<Hotkey> = None;
    let mut saw_key_down = false;

    loop {
        let event = listener
            .recv_timeout(timeout)
            .map_err(|e| format!("Timed out while waiting for shortcut input: {}", e))?;

        if event.is_key_down && event.key == Some(Key::Escape) {
            return Ok(None);
        }

        if event.is_key_down {
            let hotkey = match event.as_hotkey() {
                Ok(parsed) => parsed,
                Err(_) => continue,
            };

            if hotkey.modifiers.is_empty() && hotkey.key.is_none() {
                continue;
            }

            saw_key_down = true;
            latest_hotkey = Some(hotkey);
            continue;
        }

        if !saw_key_down {
            continue;
        }

        if event.modifiers.is_empty() {
            if let Some(recorded) = latest_hotkey {
                return Ok(Some(recorded.to_string()));
            }
        }
    }
}
