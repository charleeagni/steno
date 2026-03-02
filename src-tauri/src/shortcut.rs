use handy_keys::{Hotkey, HotkeyId, HotkeyManager, HotkeyState, Key, KeyboardListener};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::AppHandle;

pub const DEFAULT_PUSH_TO_TALK_SHORTCUT: &str = "Fn";
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "Shift+Fn";

const RESERVED_SHORTCUTS: [&str; 7] = [
    "cmd+q",
    "cmd+w",
    "cmd+tab",
    "cmd+space",
    "ctrl+space",
    "cmd+h",
    "cmd+m",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutBinding {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBindings {
    pub push_to_talk: String,
    pub toggle: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutBackendErrorCode {
    ShortcutConflict,
    InvalidShortcut,
    ShortcutModifierRequired,
    ReservedShortcut,
    ShortcutInitFailed,
}

impl ShortcutBackendErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShortcutConflict => "shortcut_conflict",
            Self::InvalidShortcut => "invalid_shortcut",
            Self::ShortcutModifierRequired => "shortcut_modifier_required",
            Self::ReservedShortcut => "reserved_shortcut",
            Self::ShortcutInitFailed => "shortcut_init_failed",
        }
    }

    pub fn guidance(self) -> &'static str {
        match self {
            Self::ShortcutConflict => "Use different shortcuts for push-to-talk and toggle.",
            Self::InvalidShortcut => "Use a valid shortcut like Shift+Fn.",
            Self::ShortcutModifierRequired => "Include a modifier key such as Shift or Ctrl.",
            Self::ReservedShortcut => "Pick a shortcut not reserved by macOS.",
            Self::ShortcutInitFailed => "Enable Input Monitoring and Accessibility, then retry.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBackendError {
    pub code: ShortcutBackendErrorCode,
    pub message: String,
}

impl ShortcutBackendError {
    fn new(code: ShortcutBackendErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type ShortcutEventHandler = fn(&AppHandle, ShortcutBinding, bool);

pub struct FnShortcutManager {
    running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
}

impl FnShortcutManager {
    pub fn start(
        app: AppHandle,
        shortcut_bindings: &[(String, ShortcutBinding)],
        event_handler: ShortcutEventHandler,
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
                    event_handler(&app, *binding, is_pressed);
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

pub fn normalize_shortcut_bindings(
    push: &str,
    toggle: &str,
) -> Result<ShortcutBindings, ShortcutBackendError> {
    // Normalize and validate both shortcut bindings.

    let push_shortcut = normalize_shortcut(push, "push-to-talk")?;
    let toggle_shortcut = normalize_shortcut(toggle, "toggle")?;

    // Reject equal shortcut assignments.

    if push_shortcut.eq_ignore_ascii_case(&toggle_shortcut) {
        return Err(ShortcutBackendError::new(
            ShortcutBackendErrorCode::ShortcutConflict,
            "Push-to-talk and toggle shortcuts must be different.",
        ));
    }

    validate_reserved_shortcut(&push_shortcut, "push-to-talk")?;
    validate_reserved_shortcut(&toggle_shortcut, "toggle")?;

    Ok(ShortcutBindings {
        push_to_talk: push_shortcut,
        toggle: toggle_shortcut,
    })
}

pub fn initialize_shortcuts_with_timeout(
    app: AppHandle,
    timeout_ms: u64,
    bindings: &ShortcutBindings,
    event_handler: ShortcutEventHandler,
) -> Result<FnShortcutManager, ShortcutBackendError> {
    // Run initialization with timeout protection.

    let (tx, rx) = std::sync::mpsc::channel();
    let push_shortcut = bindings.push_to_talk.clone();
    let toggle_shortcut = bindings.toggle.clone();

    thread::spawn(move || {
        let result = FnShortcutManager::start(
            app,
            &[
                (push_shortcut.clone(), ShortcutBinding::PushToTalk),
                (toggle_shortcut.clone(), ShortcutBinding::Toggle),
            ],
            event_handler,
        )
        .map_err(|reason| {
            ShortcutBackendError::new(
                ShortcutBackendErrorCode::ShortcutInitFailed,
                format!(
                    "Global shortcut initialization failed for push-to-talk '{}' and toggle '{}'. Details: {}",
                    push_shortcut, toggle_shortcut, reason
                ),
            )
        });
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(ShortcutBackendError::new(
            ShortcutBackendErrorCode::ShortcutInitFailed,
            format!(
                "Timed out while initializing shortcuts '{}' and '{}' after {}ms",
                bindings.push_to_talk, bindings.toggle, timeout_ms
            ),
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(ShortcutBackendError::new(
            ShortcutBackendErrorCode::ShortcutInitFailed,
            "Shortcut initializer thread disconnected",
        )),
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

fn normalize_shortcut(shortcut: &str, label: &str) -> Result<String, ShortcutBackendError> {
    let parsed = Hotkey::from_str(shortcut.trim()).map_err(|e| {
        ShortcutBackendError::new(
            ShortcutBackendErrorCode::InvalidShortcut,
            format!("Invalid {} shortcut '{}': {}", label, shortcut.trim(), e),
        )
    })?;

    if parsed.modifiers.is_empty() {
        return Err(ShortcutBackendError::new(
            ShortcutBackendErrorCode::ShortcutModifierRequired,
            format!(
                "{} shortcut '{}' must include a modifier key.",
                label,
                shortcut.trim()
            ),
        ));
    }

    Ok(parsed.to_string())
}

fn validate_reserved_shortcut(shortcut: &str, label: &str) -> Result<(), ShortcutBackendError> {
    let normalized = shortcut.to_ascii_lowercase();
    if RESERVED_SHORTCUTS.contains(&normalized.as_str()) {
        return Err(ShortcutBackendError::new(
            ShortcutBackendErrorCode::ReservedShortcut,
            format!(
                "{} shortcut '{}' is reserved by macOS and cannot be used.",
                label, shortcut
            ),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_bindings_require_unique_values() {
        let result = normalize_shortcut_bindings("Fn", "Fn");
        assert!(result.is_err());
        let err = result.expect_err("expected conflict error");
        assert_eq!(err.code, ShortcutBackendErrorCode::ShortcutConflict);
        assert_eq!(
            err.code.guidance(),
            "Use different shortcuts for push-to-talk and toggle."
        );
    }
}
