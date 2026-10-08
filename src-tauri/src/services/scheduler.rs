//! Adaptive sync: every minute while visible, every thirty minutes in the tray.

use crate::state::AppCtx;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const SYNC_INTERVAL: Duration = Duration::from_secs(1800);
const ACTIVE_SYNC_INTERVAL: Duration = Duration::from_secs(60);
const SCHEDULER_TICK: Duration = Duration::from_secs(15);

fn sync_due(elapsed: Duration, active: bool) -> bool {
    elapsed
        >= if active {
            ACTIVE_SYNC_INTERVAL
        } else {
            SYNC_INTERVAL
        }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
pub const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// Immediate sync, then adapt to panel visibility.
pub fn start(app: AppHandle) {
    let ctx = app.state::<AppCtx>();
    if let Some(old) = ctx.scheduler_task.lock().unwrap().take() {
        old.abort();
    }

    let app2 = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        crate::services::sync_engine::run_sync(app2.clone()).await;
        let mut last_attempt = now_ms();
        loop {
            tokio::time::sleep(SCHEDULER_TICK).await;
            let active = app2
                .get_webview_window(crate::panel::PANEL_LABEL)
                .is_some_and(|w| {
                    w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
                });
            let state = app2.state::<AppCtx>().sync_state.lock().unwrap().clone();
            if state.status == crate::state::SyncStatus::Syncing {
                continue;
            }
            let now = now_ms();
            // Manual syncs postpone scheduled work; failures are rate-limited too.
            let previous = last_attempt.max(state.last_sync_at.unwrap_or(0));
            if now < previous
                || sync_due(Duration::from_millis(now.saturating_sub(previous)), active)
            {
                crate::services::sync_engine::run_sync(app2.clone()).await;
                last_attempt = now_ms();
            }
        }
    });

    let ctx = app.state::<AppCtx>();
    *ctx.scheduler_task.lock().unwrap() = Some(handle);
}

pub fn stop(app: &AppHandle) {
    let ctx = app.state::<AppCtx>();
    let old = ctx.scheduler_task.lock().unwrap().take();
    if let Some(old) = old {
        old.abort();
    }
}

/// Background update poll (startup + every 24h).
pub fn start_update_checks(app: AppHandle) {
    if !crate::services::updater::available() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            let _ = crate::services::updater::check(&app).await;
            tokio::time::sleep(UPDATE_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_panel_syncs_each_minute_but_tray_keeps_background_interval() {
        assert!(!sync_due(Duration::from_secs(59), true));
        assert!(sync_due(Duration::from_secs(60), true));
        assert!(!sync_due(Duration::from_secs(60), false));
        assert!(!sync_due(Duration::from_secs(1799), false));
        assert!(sync_due(Duration::from_secs(1800), false));
    }
    #[test]
    fn stale_panel_and_resume_from_sleep_are_due() {
        assert!(sync_due(Duration::from_secs(120), true));
        assert!(sync_due(Duration::from_secs(3600), false));
    }
}
