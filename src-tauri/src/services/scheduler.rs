//! 30-minute sync scheduler — port of Services/SyncScheduler.swift, plus a
//! daily update check. Wake-from-sleep gaps are covered by the interval tick
//! comparing wall-clock time.

use crate::state::AppCtx;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const SYNC_INTERVAL: Duration = Duration::from_secs(1800);
pub const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// (Re)start the scheduler: immediate sync, then every 30 minutes.
pub fn start(app: AppHandle) {
    let ctx = app.state::<AppCtx>();
    if let Some(old) = ctx.scheduler_task.lock().unwrap().take() {
        old.abort();
    }

    let app2 = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        loop {
            crate::services::sync_engine::run_sync(app2.clone()).await;
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });

    let ctx = app.state::<AppCtx>();
    *ctx.scheduler_task.lock().unwrap() = Some(handle);
}

pub fn stop(app: &AppHandle) {
    let ctx = app.state::<AppCtx>();
    if let Some(old) = ctx.scheduler_task.lock().unwrap().take() {
        old.abort();
    }
}

/// Background update poll (startup + every 24h).
pub fn start_update_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let _ = crate::services::updater::check(&app).await;
            tokio::time::sleep(UPDATE_INTERVAL).await;
        }
    });
}
