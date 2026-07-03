//! Device-link flow — port of PopoverView.runDeviceFlow / SettingsView.relink.
//! The command returns the user code immediately; completion is pushed via
//! the `device-link` event.

use crate::services::api_client::{poll_device_code, request_device_code};
use crate::state::AppCtx;
use serde_json::json;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use vibe_core::config::VibeUsageConfig;

pub const CLIENT_NAME: &str = "Vibe Usage for Windows";

fn emit(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit("device-link", payload);
}

/// Starts the flow: requests a code, opens the browser, spawns the poll loop.
/// Returns the user code for on-screen verification.
pub async fn start(app: AppHandle) -> Result<String, String> {
    cancel(&app);

    let (http, base_url) = {
        let ctx = app.state::<AppCtx>();
        (ctx.http.clone(), ctx.config.default_api_url().to_string())
    };

    let device = request_device_code(&http, &base_url, CLIENT_NAME, AppCtx::hostname().as_deref())
        .await
        .map_err(|e| e.to_string())?;

    // System default browser (ShellExecute on Windows — NOT the CLI's broken
    // `start` path).
    let _ = open::that_detached(&device.verification_uri_complete);

    let user_code = device.user_code.clone();
    let app2 = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        poll_loop(app2, base_url, device.device_code, device.interval, device.expires_in).await;
    });

    let ctx = app.state::<AppCtx>();
    *ctx.device_link_task.lock().unwrap() = Some(handle);
    Ok(user_code)
}

pub fn cancel(app: &AppHandle) {
    let ctx = app.state::<AppCtx>();
    let handle = ctx.device_link_task.lock().unwrap().take();
    if let Some(handle) = handle {
        handle.abort();
    }
}

async fn poll_loop(app: AppHandle, base_url: String, device_code: String, interval: u64, expires_in: u64) {
    let http = {
        let ctx = app.state::<AppCtx>();
        ctx.http.clone()
    };
    let interval = Duration::from_secs(interval.max(1));
    let deadline = std::time::Instant::now() + Duration::from_secs(expires_in.max(1));

    while std::time::Instant::now() < deadline {
        tokio::time::sleep(interval).await;

        let res = match poll_device_code(&http, &base_url, &device_code).await {
            Ok(r) => r,
            // Transient network errors keep polling (mirrors `catch { continue }`).
            Err(_) => continue,
        };

        if let Some(api_key) = res.api_key {
            configure(&app, api_key, res.api_url.unwrap_or(base_url));
            emit(&app, json!({ "status": "success" }));
            return;
        }
        match res.error.as_deref() {
            Some("authorization_pending") | None => continue,
            Some("access_denied") => {
                emit(&app, json!({ "status": "denied" }));
                return;
            }
            Some("expired_token") => {
                emit(&app, json!({ "status": "expired" }));
                return;
            }
            Some(other) => {
                emit(&app, json!({ "status": "error", "message": other }));
                return;
            }
        }
    }
    emit(&app, json!({ "status": "expired" }));
}

/// Save config + start the scheduler — port of AppState.configure.
/// Hostname is stamped once at link time (the CLI reuses it on every sync).
pub fn configure(app: &AppHandle, api_key: String, api_url: String) {
    let ctx = app.state::<AppCtx>();
    let mut cfg = ctx.config.load().unwrap_or_else(VibeUsageConfig::default);
    cfg.api_key = Some(api_key);
    cfg.api_url = Some(api_url);
    if cfg.hostname.is_none() {
        cfg.hostname = AppCtx::hostname();
    }
    if let Err(e) = ctx.config.save(&cfg) {
        log::error!("config save failed: {e}");
    }
    crate::services::scheduler::start(app.clone());
}
