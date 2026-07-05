//! System tray — counterpart of Services/MenuBarController.swift.
//!
//! Left click toggles the popover panel; right click opens a native menu
//! (Windows convention). Windows shrinks tray icons aggressively, so the icon
//! stays as the high-contrast V logo and optional usage values live in the
//! tooltip.

use crate::state::{AppCtx, SyncStatus};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub const TRAY_ID: &str = "main";

pub fn tray_logo() -> tauri::image::Image<'static> {
    tauri::image::Image::new_owned(vibe_core::tray_text::render_logo_icon(32), 32, 32)
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开面板", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "立即同步", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sync, &settings, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Vibe Usage")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::panel::show(app, None),
            "sync" => {
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::services::sync_engine::run_sync(app2).await;
                });
            }
            "settings" => crate::commands::open_settings_impl(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                crate::panel::toggle_from_tray(tray.app_handle(), Some(rect));
            }
        });

    builder = builder.icon(tray_logo());
    builder.build(app)?;
    Ok(())
}

/// Refresh icon (logo vs rendered numbers) + tooltip from current state.
/// Tray mutations are proxied to the main thread — callers may be on the
/// async runtime (sync scheduler), and Windows tray icons are thread-affine.
pub fn update_tray(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || update_tray_inner(&app2));
}

fn update_tray_inner(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let ctx = app.state::<AppCtx>();

    let (show_cost, show_tokens) = {
        let s = ctx.settings.lock().unwrap();
        (s.show_cost_in_tray, s.show_tokens_in_tray)
    };
    let stats = *ctx.tray_stats.lock().unwrap();
    let sync = ctx.sync_state.lock().unwrap().clone();

    let _ = tray.set_icon(Some(tray_logo()));

    // Tooltip carries the full-precision values.
    let mut tooltip = String::from("Vibe Usage");
    if let Some((cost, tokens)) = stats {
        if show_cost {
            tooltip.push_str(&format!(" · {}", format_cost(cost)));
        }
        if show_tokens {
            tooltip.push_str(&format!(" · {} tokens", format_number(tokens)));
        }
    }
    match sync.status {
        SyncStatus::Syncing => tooltip.push_str(" · 同步中..."),
        SyncStatus::Error => {
            if let Some(m) = &sync.message {
                tooltip.push_str(&format!(" · {m}"));
            }
        }
        _ => {
            if let Some(at) = sync.last_sync_at {
                tooltip.push_str(&format!(" · 上次同步 {}", relative_time(at)));
            }
        }
    }
    let _ = tray.set_tooltip(Some(tooltip));
}

/// Mirrors Formatters.formatCost.
fn format_cost(cost: f64) -> String {
    if cost == 0.0 {
        return "$0.00".into();
    }
    if cost < 0.01 {
        return format!("${cost:.4}");
    }
    format!("${cost:.2}")
}

/// Mirrors Formatters.formatNumber.
fn format_number(n: i64) -> String {
    if n >= 1_000_000 {
        return format!("{:.1}M", n as f64 / 1e6);
    }
    if n >= 10_000 {
        return format!("{:.1}K", n as f64 / 1e3);
    }
    // thousands separators
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Mirrors Formatters.formatRelativeTime.
fn relative_time(epoch_ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let secs = now.saturating_sub(epoch_ms) / 1000;
    if secs < 60 {
        "刚刚".into()
    } else if secs < 3600 {
        format!("{} 分钟前", secs / 60)
    } else if secs < 86400 {
        format!("{} 小时前", secs / 3600)
    } else {
        format!("{} 天前", secs / 86400)
    }
}
