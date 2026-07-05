//! Tauri commands — the app's entire invoke surface (see src/lib/api.ts).

use crate::services::api_client::{self, UsageQuery};
use crate::services::{auto_launch, device_link, rate_limits, scheduler, sync_engine, updater};
use crate::state::{AppCtx, AppSettings, SyncState, UpdateInfo};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use vibe_core::ProviderRateLimit;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    configured: bool,
    api_url: String,
    version: String,
    is_dev: bool,
    runtime_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key_display: Option<String>,
}

#[tauri::command]
pub fn get_app_status(app: AppHandle) -> AppStatus {
    let ctx = app.state::<AppCtx>();
    let config = ctx.config.load();
    let api_key = config.as_ref().and_then(|c| c.api_key.clone());
    let api_url = config
        .as_ref()
        .and_then(|c| c.api_url.clone())
        .unwrap_or_else(|| ctx.config.default_api_url().to_string());

    let api_key_display = api_key.as_ref().map(|key| {
        if key.chars().count() > 12 {
            let prefix: String = key.chars().take(8).collect();
            let suffix: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("{prefix}...{suffix}")
        } else {
            key.clone()
        }
    });

    AppStatus {
        configured: api_key.is_some(),
        api_url,
        version: app.package_info().version.to_string(),
        is_dev: crate::state::IS_DEV,
        runtime_available: sync_engine::detect_runtime(&app).is_some(),
        api_key_display,
    }
}

#[tauri::command]
pub async fn fetch_usage(app: AppHandle, query: UsageQuery) -> Result<Value, String> {
    let (http, base_url, api_key) = {
        let ctx = app.state::<AppCtx>();
        let config = ctx.config.load().ok_or("未配置")?;
        let api_key = config.api_key.ok_or("未配置")?;
        let base_url = config
            .api_url
            .unwrap_or_else(|| ctx.config.default_api_url().to_string());
        (ctx.http.clone(), base_url, api_key)
    };
    api_client::fetch_usage(&http, &base_url, &api_key, &query)
        .await
        .map_err(|e| e.to_string())
}

// -- Device link --------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLinkStart {
    user_code: String,
}

#[tauri::command]
pub async fn start_device_link(app: AppHandle) -> Result<DeviceLinkStart, String> {
    device_link::start(app).await.map(|user_code| DeviceLinkStart { user_code })
}

#[tauri::command]
pub fn cancel_device_link(app: AppHandle) {
    device_link::cancel(&app);
}

/// CI / no-browser fallback — validates the pre-issued key with a live
/// `GET /api/usage?days=1` before saving (mirrors CLI --manual-key intent).
#[tauri::command]
pub async fn set_manual_key(app: AppHandle, api_key: String) -> Result<(), String> {
    let api_key = api_key.trim().to_string();
    if !api_key.starts_with("vbu_") {
        return Err("API Key 必须以 vbu_ 开头".into());
    }
    let (http, base_url) = {
        let ctx = app.state::<AppCtx>();
        (ctx.http.clone(), ctx.config.default_api_url().to_string())
    };
    api_client::fetch_usage(&http, &base_url, &api_key, &UsageQuery::Days { days: 1 })
        .await
        .map_err(|e| e.to_string())?;
    device_link::configure(&app, api_key, base_url);
    Ok(())
}

// -- Sync ---------------------------------------------------------------------

#[tauri::command]
pub fn trigger_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        sync_engine::run_sync(app).await;
    });
}

#[tauri::command]
pub fn get_sync_state(app: AppHandle) -> SyncState {
    app.state::<AppCtx>().sync_state.lock().unwrap().clone()
}

// -- Rate limits ----------------------------------------------------------------

#[tauri::command]
pub async fn get_rate_limits(app: AppHandle, force: bool) -> Vec<ProviderRateLimit> {
    rate_limits::get_rate_limits(&app, force).await
}

#[tauri::command]
pub async fn enable_claude_rate_limit(app: AppHandle) -> Result<Vec<ProviderRateLimit>, String> {
    rate_limits::enable_claude(&app).await
}

// -- Settings -------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    app.state::<AppCtx>().settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: AppSettings) {
    let claude_was_enabled = {
        let ctx = app.state::<AppCtx>();
        let mut current = ctx.settings.lock().unwrap();
        let was = current.claude_rate_limit_enabled;
        *current = settings.clone();
        was
    };
    let ctx = app.state::<AppCtx>();
    ctx.save_settings();

    // Disabling Claude capture restores the user's original statusline.
    if claude_was_enabled && !settings.claude_rate_limit_enabled {
        let _ = rate_limits::statusline_hook(&app).uninstall();
    }
    let _ = app.emit("settings-updated", &settings);
    crate::tray::update_tray(&app);
}

#[tauri::command]
pub fn get_launch_at_login() -> Result<bool, String> {
    auto_launch::get()
}

#[tauri::command]
pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    auto_launch::set(enabled)
}

#[tauri::command]
pub fn reset_config(app: AppHandle) -> Result<(), String> {
    scheduler::stop(&app);
    let ctx = app.state::<AppCtx>();
    ctx.config.reset().map_err(|e| e.to_string())?;
    *ctx.tray_stats.lock().unwrap() = None;
    crate::tray::update_tray(&app);
    Ok(())
}

// -- Shell / windows ---------------------------------------------------------------

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("仅允许打开 http(s) 链接".into());
    }
    crate::process_utils::shell_open(&url)
}

pub fn open_settings_impl(app: &AppHandle) {
    if let Some(existing) = app.get_webview_window("settings") {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    // The packaged app declares a hidden settings window in tauri.conf.json so
    // WebView has loaded before the tray menu asks to show it. Keep this as a
    // recovery path in case the window was closed by the platform.
    // Keep the app URL query-free here: packaged asset loading treats the
    // whole string as an app resource path on some WebView/Tauri versions.
    let result = WebviewWindowBuilder::new(
        app,
        "settings",
        WebviewUrl::App("index.html".into()),
    )
        .title("Vibe Usage 设置")
        .inner_size(460.0, 620.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
    if let Err(e) = result {
        log::error!("settings window: {e}");
    }
}

#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    open_settings_impl(&app);
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    crate::panel::hide_now(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

// -- Tray ------------------------------------------------------------------------

/// Pushed by the frontend after each fetch/range change: cost + tokens for
/// the ACTIVE time range (no filters) — mirrors menuBarCost/menuBarTokens.
#[tauri::command]
pub fn update_tray_stats(app: AppHandle, cost: f64, tokens: i64) {
    {
        let ctx = app.state::<AppCtx>();
        *ctx.tray_stats.lock().unwrap() = Some((cost, tokens));
    }
    crate::tray::update_tray(&app);
}

// -- Updates -----------------------------------------------------------------------

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    updater::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(&app).await
}
