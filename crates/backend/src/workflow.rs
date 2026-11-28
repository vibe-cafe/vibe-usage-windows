use crate::services::{
    api_client, cursor_login, diagnostics, rate_limits, sync_engine, sync_history::SyncTrigger,
    updater,
};
use crate::{Backend, UpdateInfo, UsageQuery};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;
use vibe_core::quota_product::ZCodeQuotaRegion;
use vibe_core::{ProviderRateLimit, RateLimitProvider};

// Clock and I/O boundaries: tests drive Tokio's paused clock and record the
// operations which would have reached the client or a managed subprocess.
#[async_trait]
pub(crate) trait Network: Send + Sync {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
    async fn proxy_ready(&self, app: &Backend);
    async fn usage(
        &self,
        app: &Backend,
        url: &str,
        key: &str,
        query: &UsageQuery,
    ) -> Result<Value, String>;
    async fn quotas(
        &self,
        app: &Backend,
        due: &[RateLimitProvider],
        region: ZCodeQuotaRegion,
    ) -> Vec<ProviderRateLimit>;
    async fn sync(&self, app: &Backend) -> sync_engine::CliRun;
    async fn update(&self, app: &Backend) -> Result<Option<UpdateInfo>, String>;
    fn cursor_support(&self, app: &Backend);
    async fn diagnostics(&self, app: &Backend) -> String;
}

pub(crate) struct Live;

#[cfg(test)]
mod tests;

#[async_trait]
impl Network for Live {
    async fn proxy_ready(&self, app: &Backend) {
        let urls = crate::system_proxy::launch_probe_urls(&app.ctx().config.load_api_url());
        if !crate::system_proxy::wait_for_launch_proxy(urls).await {
            log::info!("login launch: proxy wait reached its deadline");
        }
    }
    async fn usage(
        &self,
        app: &Backend,
        url: &str,
        key: &str,
        query: &UsageQuery,
    ) -> Result<Value, String> {
        api_client::fetch_usage(&app.ctx().http, url, key, query)
            .await
            .map_err(|e| e.to_string())
    }
    async fn quotas(
        &self,
        app: &Backend,
        due: &[RateLimitProvider],
        region: ZCodeQuotaRegion,
    ) -> Vec<ProviderRateLimit> {
        rate_limits::read_due(app, due, region).await
    }
    async fn sync(&self, app: &Backend) -> sync_engine::CliRun {
        sync_engine::run_cli_sync(app).await
    }
    async fn update(&self, app: &Backend) -> Result<Option<UpdateInfo>, String> {
        updater::check(app).await
    }
    fn cursor_support(&self, app: &Backend) {
        cursor_login::check_support(app);
    }
    async fn diagnostics(&self, app: &Backend) -> String {
        diagnostics::text(&diagnostics::collect(app).await)
    }
}

#[derive(Clone, Serialize)]
pub struct UsageSnapshot {
    pub query: UsageQuery,
    pub data: Value,
}

struct Recent {
    account: (String, String),
    snapshot: UsageSnapshot,
    at: Instant,
}

pub(crate) struct Usage {
    query: Mutex<UsageQuery>,
    recent: Mutex<Option<Recent>>,
    reading: tokio::sync::Mutex<()>,
}

impl Default for Usage {
    fn default() -> Self {
        Self {
            query: Mutex::new(UsageQuery::Days { days: 1 }),
            recent: Default::default(),
            reading: Default::default(),
        }
    }
}

impl Backend {
    pub(crate) fn start_network(&self) {
        let linked = self.ctx().config.is_configured();
        if linked {
            crate::services::scheduler::start(self.clone());
        }
        if updater::available() {
            let app = self.clone();
            self.spawn(async move {
                loop {
                    let _ = app.0.network.update(&app).await;
                    tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
                }
            });
        }
        let quotas = !self
            .ctx()
            .settings
            .lock()
            .unwrap()
            .selected_quota_product_ids
            .is_empty();
        if self.options().autostart && (linked || quotas) {
            let app = self.clone();
            self.spawn(async move {
                app.0.network.proxy_ready(&app).await;
                app.ctx().launch_network.send_replace(true);
            });
        } else {
            self.ctx().launch_network.send_replace(true);
        }
        if linked {
            self.refresh_usage(false);
        }
        if quotas {
            self.refresh_quotas();
        }
    }

    fn account(&self) -> Option<(String, String)> {
        api_client::linked_account(&self.ctx().config)
    }

    pub fn cached_usage(&self) -> Option<UsageSnapshot> {
        let account = self.account()?;
        self.0
            .usage
            .recent
            .lock()
            .unwrap()
            .as_ref()
            .filter(|r| r.account == account)
            .map(|r| r.snapshot.clone())
    }

    pub fn panel_opened(&self, query: UsageQuery) {
        *self.0.usage.query.lock().unwrap() = query;
        self.refresh_usage(false);
        self.refresh_quotas();
    }

    // Closing or preloading a window, showing About and reading settings
    // are local operations. There is no visibility-driven poller.
    pub fn settings_shown(&self) {
        let app = self.clone();
        self.spawn(async move {
            app.ctx().wait_launch_network().await;
            app.0.network.cursor_support(&app);
        });
    }

    pub(crate) fn refresh_usage(&self, force: bool) {
        let query = self.0.usage.query.lock().unwrap().clone();
        let app = self.clone();
        self.spawn(async move {
            if let Err(error) = app.read_usage(&query, force).await {
                if app.ctx().config.is_configured() {
                    let _ = app.emit(
                        "usage-error",
                        serde_json::json!({ "query": query, "error": error }),
                    );
                }
            }
        });
    }

    async fn read_usage(&self, query: &UsageQuery, force: bool) -> Result<Value, String> {
        self.ctx().wait_launch_network().await;
        let _reading = self.0.usage.reading.lock().await;
        let generation = *self.ctx().account_generation.lock().unwrap();
        let account = self.account().ok_or("未配置")?;
        if !force {
            if let Some(recent) = self.0.usage.recent.lock().unwrap().as_ref() {
                if recent.account == account
                    && &recent.snapshot.query == query
                    && recent.at.elapsed() < Duration::from_secs(60)
                {
                    return Ok(recent.snapshot.data.clone());
                }
            }
        }
        let data = self
            .0
            .network
            .usage(self, &account.0, &account.1, query)
            .await?;
        let snapshot = UsageSnapshot {
            query: query.clone(),
            data: data.clone(),
        };
        {
            let current = self.ctx().account_generation.lock().unwrap();
            if *current != generation || self.account().as_ref() != Some(&account) {
                return Err("账号已更改".into());
            }
            *self.0.usage.recent.lock().unwrap() = Some(Recent {
                account,
                snapshot: snapshot.clone(),
                at: Instant::now(),
            });
            let _ = self.emit("usage-updated", snapshot);
        }
        Ok(data)
    }

    pub(crate) async fn select_usage(&self, query: UsageQuery) -> Result<Value, String> {
        *self.0.usage.query.lock().unwrap() = query.clone();
        self.read_usage(&query, false).await
    }

    pub(crate) async fn quotas(&self, force: bool) -> Vec<ProviderRateLimit> {
        rate_limits::get_rate_limits(self, force).await
    }

    pub(crate) fn refresh_quotas(&self) {
        let app = self.clone();
        self.spawn(async move {
            app.quotas(false).await;
        });
    }

    pub(crate) async fn sync(&self, trigger: SyncTrigger) {
        sync_engine::run_sync(self.clone(), trigger).await;
    }

    pub(crate) fn reset_sync_state(&self) -> Result<(), String> {
        self.ctx()
            .update_settings(|s| s.last_successful_sync_at = None)?;
        *self.ctx().sync_state.lock().unwrap() = Default::default();
        *self.0.usage.recent.lock().unwrap() = None;
        Ok(())
    }
}
