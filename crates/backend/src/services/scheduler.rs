//! Immediate sync after linking, then at most once per thirty minutes.
use super::sync_history::SyncTrigger;
use crate::Backend;
use std::time::Duration;

const INTERVAL_MS: u64 = 30 * 60 * 1000;
const HEARTBEAT: Duration = Duration::from_secs(15);

pub fn start(app: Backend) {
    let mut task = app.ctx().scheduler_task.lock().unwrap();
    if let Some(old) = task.take() {
        old.abort();
    }
    let worker = app.clone();
    *task = app.spawn(async move {
        worker.ctx().wait_launch_network().await;
        worker.sync(SyncTrigger::Scheduled).await;
        let mut attempted = worker.0.network.now_ms();
        // Windows timer waits can stop during sleep. Check wall time again
        // within one heartbeat after resume, even when the panel stays open.
        let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticks.tick().await;
            let now = worker.0.network.now_ms();
            let previous = {
                let state = worker.ctx().sync_state.lock().unwrap();
                if state.status == crate::SyncStatus::Syncing {
                    continue;
                }
                // A successful manual sync postpones the next attempt.
                attempted.max(state.last_sync_at.filter(|&last| last <= now).unwrap_or(0))
            };
            if now < attempted || now.saturating_sub(previous) >= INTERVAL_MS {
                worker.sync(SyncTrigger::Scheduled).await;
                attempted = worker.0.network.now_ms();
            }
        }
    });
}

pub fn stop(app: &Backend) {
    if let Some(task) = app.ctx().scheduler_task.lock().unwrap().take() {
        task.abort();
    }
}
