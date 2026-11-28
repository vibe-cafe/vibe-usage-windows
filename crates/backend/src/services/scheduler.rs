//! Immediate sync after linking, then at most once per thirty minutes.
use super::sync_history::SyncTrigger;
use crate::Backend;
use std::time::Duration;

const INTERVAL: Duration = Duration::from_secs(30 * 60);

pub fn start(app: Backend) {
    let mut task = app.ctx().scheduler_task.lock().unwrap();
    if let Some(old) = task.take() {
        old.abort();
    }
    let worker = app.clone();
    *task = app.spawn(async move {
        worker.ctx().wait_launch_network().await;
        loop {
            worker.sync(SyncTrigger::Scheduled).await;
            let attempted = tokio::time::Instant::now();
            loop {
                // A successful manual sync also postpones scheduled work.
                let (last, syncing) = {
                    let state = worker.ctx().sync_state.lock().unwrap();
                    (
                        state.last_success,
                        state.status == crate::SyncStatus::Syncing,
                    )
                };
                let deadline = last.unwrap_or(attempted).max(attempted) + INTERVAL;
                if deadline <= tokio::time::Instant::now() {
                    if !syncing {
                        break;
                    }
                    // The manual run already covers this tick. Recheck its
                    // outcome instead of queueing an immediate second upload.
                    tokio::time::sleep(Duration::from_secs(15)).await;
                } else {
                    tokio::time::sleep_until(deadline).await;
                }
            }
        }
    });
}

pub fn stop(app: &Backend) {
    if let Some(task) = app.ctx().scheduler_task.lock().unwrap().take() {
        task.abort();
    }
}
