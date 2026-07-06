//! Main panel window lifecycle.

use tauri::{AppHandle, Emitter, Manager};

pub const PANEL_LABEL: &str = "popover";

/// Show the normal app window and bring it to the foreground. If it was hidden
/// to the tray, center it so a relaunch has a predictable position.
pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    let was_visible = window.is_visible().unwrap_or(false);
    let _ = window.unminimize();
    if !was_visible {
        let _ = window.center();
    }
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("panel-shown", ());
}

/// Keep the tray app alive when users close the main window.
pub fn hide_now(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        let _ = window.hide();
    }
}
