//! Popover panel show/hide/positioning — counterpart of MenuBarController's
//! panel lifecycle. The open/close animation itself runs in the WebView
//! (styles/globals.css); Rust owns geometry and visibility.

use crate::state::AppCtx;
use serde_json::json;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect};

pub const PANEL_LABEL: &str = "popover";
pub const PANEL_W: f64 = 520.0;
pub const PANEL_H: f64 = 620.0;
/// Gap between the tray icon and the panel (macOS panelTopGap).
pub const PANEL_GAP: f64 = 6.0;
/// A tray click right after a blur-hide must not instantly reopen the panel.
const REOPEN_SUPPRESS: Duration = Duration::from_millis(300);

pub fn toggle_from_tray(app: &AppHandle, tray_rect: Option<Rect>) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        begin_hide(app);
        return;
    }
    // Blur-hide races the click: the panel already closed because this very
    // click stole focus. Reopening now would make it look unclosable.
    {
        let ctx = app.state::<AppCtx>();
        let last_hide = *ctx.last_panel_hide.lock().unwrap();
        if let Some(at) = last_hide {
            if at.elapsed() < REOPEN_SUPPRESS {
                return;
            }
        }
    }
    show(app, tray_rect);
}

/// Position (right-aligned to the tray icon, above/below the taskbar) + show.
pub fn show(app: &AppHandle, tray_rect: Option<Rect>) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };

    let mut origin = "top right";
    if let Some(rect) = tray_rect {
        let (rx, ry) = match rect.position {
            tauri::Position::Physical(p) => (p.x as f64, p.y as f64),
            tauri::Position::Logical(p) => (p.x, p.y),
        };
        let (rw, rh) = match rect.size {
            tauri::Size::Physical(s) => (s.width as f64, s.height as f64),
            tauri::Size::Logical(s) => (s.width, s.height),
        };

        let monitor = window
            .monitor_from_point(rx, ry)
            .ok()
            .flatten()
            .or_else(|| window.primary_monitor().ok().flatten());

        if let Some(monitor) = monitor {
            let scale = monitor.scale_factor();
            let panel_w = PANEL_W * scale;
            let panel_h = PANEL_H * scale;
            let gap = PANEL_GAP * scale;
            let margin = 8.0 * scale;
            let mon_pos = monitor.position();
            let mon_size = monitor.size();
            let mon_x = mon_pos.x as f64;
            let mon_y = mon_pos.y as f64;
            let mon_w = mon_size.width as f64;
            let mon_h = mon_size.height as f64;

            // Right edge of panel anchored to the tray icon's right edge.
            let mut x = rx + rw - panel_w;
            x = x.clamp(mon_x + margin, (mon_x + mon_w - panel_w - margin).max(mon_x + margin));

            // Taskbar at the bottom (usual) → panel opens upward from the icon.
            let tray_at_bottom = ry > mon_y + mon_h / 2.0;
            let y = if tray_at_bottom {
                origin = "bottom right";
                (ry - panel_h - gap).max(mon_y + margin)
            } else {
                origin = "top right";
                ry + rh + gap
            };

            let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
        }
    }

    // panel-shown drives the open animation + popover-open refreshes
    // (usage 60s debounce, rate limits 60s debounce — handled frontend-side).
    let _ = app.emit("panel-shown", json!({ "origin": origin }));
    let _ = window.show();
    let _ = window.set_focus();
    start_dismiss_watch(app);
}

/// Blur-based dismissal is unreliable on Windows: foreground-activation rules
/// can deny `set_focus` for a tray popup, and a window that never had focus
/// never fires Focused(false) — leaving an always-on-top panel stuck over
/// everything (the classic tray-popover bug).
///
/// While the panel is visible, poll `GetForegroundWindow` (a benign,
/// read-only API — deliberately NOT input hooks/GetAsyncKeyState, which
/// pattern-match keylogger heuristics in Windows Defender):
///   - once the panel has been foreground and the foreground moves elsewhere
///     → dismiss (classic popover behavior);
///   - if activation was denied and the panel was never foreground, a CHANGE
///     of foreground away from whatever was active at open → dismiss too.
/// The Focused(false) handler remains as the fast path.
fn start_dismiss_watch(app: &AppHandle) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Give show() → activation a moment to settle.
            tokio::time::sleep(Duration::from_millis(400)).await;
            let initial_fg = unsafe { GetForegroundWindow() } as usize;
            let mut was_ours = false;
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                let Some(window) = app.get_webview_window(PANEL_LABEL) else {
                    return;
                };
                if !window.is_visible().unwrap_or(false) {
                    return;
                }
                let ours = match window.hwnd() {
                    Ok(h) => h.0 as usize,
                    Err(_) => return,
                };
                let fg = unsafe { GetForegroundWindow() } as usize;
                if fg == ours {
                    was_ours = true;
                } else if was_ours || (fg != initial_fg && fg != 0) {
                    begin_hide(&app);
                    return;
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

/// Ask the frontend to play the close animation, then force-hide as a
/// fallback in case the WebView is unresponsive (anim is 140ms).
pub fn begin_hide(app: &AppHandle) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    {
        let ctx = app.state::<AppCtx>();
        *ctx.last_panel_hide.lock().unwrap() = Some(Instant::now());
    }
    let _ = app.emit("panel-will-hide", ());
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        if let Some(w) = app2.get_webview_window(PANEL_LABEL) {
            if w.is_visible().unwrap_or(false) {
                let _ = w.hide();
            }
        }
    });
}

/// Immediate hide — invoked by the frontend after its close animation.
pub fn hide_now(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        let _ = window.hide();
    }
}
