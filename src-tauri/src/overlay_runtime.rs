use tauri::{AppHandle, Manager, PhysicalPosition, Position, WebviewWindow};

const OVERLAY_WINDOW_LABEL: &str = "overlay";
const DEFAULT_OVERLAY_BOTTOM_MARGIN_PX: i32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayRuntimePhase {
    Idle,
    Recording,
    Processing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayRenderContext {
    pub phase: OverlayRuntimePhase,
}

pub type OverlayContentSlot = fn(&AppHandle, &OverlayRenderContext);

pub struct OverlayRuntimeController {
    content_slot: OverlayContentSlot,
    bottom_margin_px: i32,
}

impl OverlayRuntimeController {
    pub fn new(content_slot: OverlayContentSlot) -> Self {
        Self {
            content_slot,
            bottom_margin_px: DEFAULT_OVERLAY_BOTTOM_MARGIN_PX,
        }
    }

    pub fn sync_overlay_shell(&self, app: &AppHandle, phase: OverlayRuntimePhase) {
        // Render shell context and consumer slot.

        (self.content_slot)(app, &OverlayRenderContext { phase });

        let Some(overlay_window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
            return;
        };

        match phase {
            OverlayRuntimePhase::Recording | OverlayRuntimePhase::Processing => {
                self.position_overlay_window(app, &overlay_window);
                if let Err(e) = overlay_window.show() {
                    log::error!("event=overlay_show_failed error={}", e);
                }
            }
            OverlayRuntimePhase::Idle | OverlayRuntimePhase::Error => {
                if let Err(e) = overlay_window.hide() {
                    log::error!("event=overlay_hide_failed error={}", e);
                }
            }
        }
    }

    fn position_overlay_window(&self, app: &AppHandle, overlay_window: &WebviewWindow) {
        let Ok(window_size) = overlay_window.outer_size() else {
            return;
        };

        // Prefer the monitor containing the cursor.

        let monitor_at_cursor = app.cursor_position().ok().and_then(|cursor| {
            app.available_monitors().ok().and_then(|monitors| {
                let contains_point = |monitor: &tauri::Monitor, use_logical_bounds: bool| {
                    let position = monitor.position();
                    let size = monitor.size();
                    let scale_factor = monitor.scale_factor().max(1.0);

                    let left = if use_logical_bounds {
                        position.x as f64 / scale_factor
                    } else {
                        position.x as f64
                    };
                    let top = if use_logical_bounds {
                        position.y as f64 / scale_factor
                    } else {
                        position.y as f64
                    };
                    let right = if use_logical_bounds {
                        left + (size.width as f64 / scale_factor)
                    } else {
                        left + size.width as f64
                    };
                    let bottom = if use_logical_bounds {
                        top + (size.height as f64 / scale_factor)
                    } else {
                        top + size.height as f64
                    };

                    cursor.x >= left && cursor.x < right && cursor.y >= top && cursor.y < bottom
                };

                let squared_distance = |monitor: &tauri::Monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    let scale_factor = monitor.scale_factor().max(1.0);
                    let physical_center = (
                        position.x as f64 + (size.width as f64 / 2.0),
                        position.y as f64 + (size.height as f64 / 2.0),
                    );
                    let logical_center = (
                        position.x as f64 / scale_factor + (size.width as f64 / scale_factor / 2.0),
                        position.y as f64 / scale_factor
                            + (size.height as f64 / scale_factor / 2.0),
                    );
                    let physical_distance = (cursor.x - physical_center.0).powi(2)
                        + (cursor.y - physical_center.1).powi(2);
                    let logical_distance = (cursor.x - logical_center.0).powi(2)
                        + (cursor.y - logical_center.1).powi(2);
                    physical_distance.min(logical_distance)
                };

                let physical_match = monitors
                    .iter()
                    .find(|monitor| contains_point(monitor, false))
                    .cloned();
                let logical_match = monitors
                    .iter()
                    .find(|monitor| contains_point(monitor, true))
                    .cloned();
                let nearest_match = monitors
                    .iter()
                    .min_by(|left_monitor, right_monitor| {
                        squared_distance(left_monitor).total_cmp(&squared_distance(right_monitor))
                    })
                    .cloned();

                app.monitor_from_point(cursor.x, cursor.y)
                    .ok()
                    .flatten()
                    .or(physical_match)
                    .or(logical_match)
                    .or(nearest_match)
            })
        });

        let target_monitor = monitor_at_cursor
            .or_else(|| overlay_window.current_monitor().ok().flatten())
            .or_else(|| overlay_window.primary_monitor().ok().flatten());

        let Some(monitor) = target_monitor else {
            return;
        };

        let work_area = monitor.work_area();
        let x =
            work_area.position.x + ((work_area.size.width as i32 - window_size.width as i32) / 2);
        let y = (work_area.position.y + work_area.size.height as i32
            - window_size.height as i32
            - self.bottom_margin_px)
            .max(work_area.position.y);

        if let Err(e) = overlay_window.set_position(Position::Physical(PhysicalPosition::new(x, y)))
        {
            log::error!("event=overlay_position_failed error={}", e);
        }
    }
}

fn noop_overlay_content_slot(_: &AppHandle, _: &OverlayRenderContext) {}

impl Default for OverlayRuntimeController {
    fn default() -> Self {
        Self::new(noop_overlay_content_slot)
    }
}
