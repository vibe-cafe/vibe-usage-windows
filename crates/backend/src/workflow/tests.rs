use super::*;
use crate::commands::{self, Setting};
use crate::services::{
    cursor_login::{LoginState, LoginStatus},
    sync_history::SyncOutcome,
};
use crate::{Host, Options};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tempfile::TempDir;
use tokio::sync::Notify;
use vibe_core::{
    config::{ConfigManager, VibeUsageConfig},
    RateLimitStatus,
};
use RateLimitProvider::{Cursor, Grok};

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Proxy,
    Usage(UsageQuery),
    Quotas(Vec<RateLimitProvider>),
    Sync,
    Update,
    Support,
    Diagnostics,
}

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<Call>>,
    proxy: Notify,
    success: AtomicBool,
    hold_sync: AtomicBool,
    finish_sync: Notify,
}

impl Fake {
    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }
    fn take(&self) -> Vec<Call> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

#[async_trait]
impl Network for Fake {
    async fn proxy_ready(&self, _: &Backend) {
        self.record(Call::Proxy);
        self.proxy.notified().await;
    }
    async fn usage(
        &self,
        _: &Backend,
        _: &str,
        _: &str,
        query: &UsageQuery,
    ) -> Result<Value, String> {
        self.record(Call::Usage(query.clone()));
        Ok(serde_json::json!({"buckets":[]}))
    }
    async fn quotas(
        &self,
        _: &Backend,
        due: &[RateLimitProvider],
        _: ZCodeQuotaRegion,
    ) -> Vec<ProviderRateLimit> {
        self.record(Call::Quotas(due.to_vec()));
        due.iter()
            .map(|p| ProviderRateLimit::empty(*p, RateLimitStatus::Ok))
            .collect()
    }
    async fn sync(&self, _: &Backend) -> sync_engine::CliRun {
        self.record(Call::Sync);
        if self.hold_sync.load(Ordering::SeqCst) {
            self.finish_sync.notified().await;
        }
        let success = self.success.load(Ordering::SeqCst);
        sync_engine::CliRun {
            message: if success {
                Ok("ok".into())
            } else {
                Err("fixture".into())
            },
            outcome: if success {
                SyncOutcome::Success
            } else {
                SyncOutcome::Failed
            },
            reason: None,
            cli_fallback: None,
        }
    }
    async fn update(&self, _: &Backend) -> Result<Option<UpdateInfo>, String> {
        self.record(Call::Update);
        Ok(None)
    }
    fn cursor_support(&self, _: &Backend) {
        self.record(Call::Support);
    }
    async fn diagnostics(&self, _: &Backend) -> String {
        self.record(Call::Diagnostics);
        "synthetic report".into()
    }
}

struct TestHost;
impl Host for TestHost {
    fn event(&self, _: &str, _: Value) {}
    fn request_exit(&self) {}
}

struct Fixture {
    dir: TempDir,
    app: Backend,
    net: Arc<Fake>,
}

impl Fixture {
    fn new(autostart: bool, linked: bool, selected: Vec<RateLimitProvider>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigManager::with_dir(dir.path().join("cli"), false);
        if linked {
            config
                .save(&VibeUsageConfig {
                    api_url: Some("https://example.invalid".into()),
                    api_key: Some("fixture".into()),
                    ..Default::default()
                })
                .unwrap();
        }
        let options = Options {
            roaming_config: dir.path().join("roaming"),
            local_data: dir.path().join("local"),
            resources: dir.path().join("resources"),
            version: "1.0".into(),
            autostart,
        };
        let net = Arc::new(Fake::default());
        let app =
            Backend::initialize_with(options, Arc::new(TestHost), config, net.clone(), |app| {
                assert_eq!(app.ctx().launch_network_ready(), !autostart);
                app.ctx()
                    .settings
                    .lock()
                    .unwrap()
                    .set_quota_selection(selected);
            });
        Self { dir, app, net }
    }
    fn reload(&self) -> crate::state::AppCtx {
        crate::state::AppCtx::new(self.dir.path().join("roaming"))
    }
}

async fn settle() {
    // All fake operations complete on this runtime, apart from the explicit proxy wait.
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

async fn advance(seconds: u64) {
    tokio::time::advance(Duration::from_secs(seconds)).await;
    settle().await;
}

fn today() -> UsageQuery {
    UsageQuery::Days { days: 1 }
}

#[tokio::test(start_paused = true)]
async fn autostart_blocks_every_reader_and_manual_sync_but_not_updates() {
    let f = Fixture::new(true, true, vec![Cursor]);
    let app = &f.app;
    assert!(!app.ctx().launch_network_ready());
    commands::trigger_sync(app.clone());
    app.panel_opened(today());
    app.settings_shown();
    commands::update_setting(app.clone(), Setting::ShowCostInTray(false)).unwrap();
    let doctor = tokio::spawn(commands::collect_diagnostics(app.clone()));
    let quota = tokio::spawn(commands::get_rate_limits(app.clone(), false));
    let usage = tokio::spawn(commands::fetch_usage(app.clone(), today()));
    advance(90).await;
    let calls = f.net.take();
    assert!(calls.contains(&Call::Proxy));
    assert_eq!(calls.contains(&Call::Update), updater::available());
    assert!(calls
        .iter()
        .all(|c| matches!(c, Call::Proxy | Call::Update)));
    assert!(!doctor.is_finished() && !quota.is_finished() && !usage.is_finished());
    f.net.proxy.notify_one();
    doctor.await.unwrap();
    quota.await.unwrap();
    usage.await.unwrap().unwrap();
    settle().await;
    let calls = f.net.take();
    assert!(
        calls.contains(&Call::Sync)
            && calls.contains(&Call::Support)
            && calls.contains(&Call::Diagnostics)
    );
    assert_eq!(
        calls.iter().filter(|c| matches!(c, Call::Usage(_))).count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|c| matches!(c, Call::Quotas(_)))
            .count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn startup_reads_once_and_only_panel_opening_checks_product_intervals() {
    let f = Fixture::new(false, true, vec![Cursor, Grok]);
    settle().await;
    let mut calls = f.net.take();
    calls.retain(|c| *c != Call::Update);
    assert_eq!(calls.len(), 3);
    assert!(calls.contains(&Call::Sync) && calls.contains(&Call::Usage(today())));
    assert!(calls.contains(&Call::Quotas(vec![Cursor, Grok])));
    f.app.panel_opened(today());
    settle().await;
    advance(61).await;
    assert!(f.net.take().is_empty());
    f.app.panel_opened(today());
    settle().await;
    let calls = f.net.take();
    assert_eq!(calls.len(), 2);
    assert!(calls.contains(&Call::Usage(today())) && calls.contains(&Call::Quotas(vec![Cursor])));
    advance(1738).await;
    assert!(f.net.take().is_empty());
    advance(1).await;
    assert_eq!(f.net.take(), [Call::Sync]);
    f.app.panel_opened(today());
    settle().await;
    assert!(f.net.take().contains(&Call::Quotas(vec![Cursor, Grok])));
}

#[tokio::test(start_paused = true)]
async fn success_rereads_usage_only_and_survives_restart_until_reset() {
    let f = Fixture::new(false, true, vec![Cursor]);
    settle().await;
    f.net.take();
    assert!(f.reload().sync_state.lock().unwrap().last_sync_at.is_none());
    f.net.success.store(true, Ordering::SeqCst);
    commands::trigger_sync(f.app.clone());
    settle().await;
    assert_eq!(f.net.take(), [Call::Sync, Call::Usage(today())]);
    let last = commands::get_sync_state(f.app.clone())
        .last_sync_at
        .unwrap();
    assert_eq!(
        f.reload().sync_state.lock().unwrap().last_sync_at,
        Some(last)
    );
    assert_eq!(commands::get_sync_history(f.app.clone()).len(), 2);
    commands::reset_config(f.app.clone()).unwrap();
    assert_eq!(f.reload().sync_state.lock().unwrap().last_sync_at, None);
    assert!(commands::get_sync_history(f.app.clone()).is_empty());
    assert!(f.app.cached_usage().is_none());
    advance(1800).await;
    assert!(f.net.take().is_empty());
}

#[tokio::test(start_paused = true)]
async fn manual_sync_across_a_scheduled_tick_does_not_queue_another_upload() {
    let f = Fixture::new(false, true, vec![]);
    settle().await;
    f.net.take();
    advance(1799).await;
    f.net.hold_sync.store(true, Ordering::SeqCst);
    f.net.success.store(true, Ordering::SeqCst);
    commands::trigger_sync(f.app.clone());
    settle().await;
    assert_eq!(f.net.take(), [Call::Sync]);
    advance(1).await;
    f.net.hold_sync.store(false, Ordering::SeqCst);
    f.net.finish_sync.notify_one();
    settle().await;
    advance(15).await;
    assert_eq!(f.net.take(), [Call::Usage(today())]);
}

#[tokio::test(start_paused = true)]
async fn settings_patches_preserve_order_and_cursor_changes_refresh_selected_cards() {
    let f = Fixture::new(false, false, vec![Cursor]);
    settle().await;
    f.net.take();
    commands::update_setting(
        f.app.clone(),
        Setting::QuotaProductOrder(vec![Grok, Cursor, Grok]),
    )
    .unwrap();
    commands::update_setting(f.app.clone(), Setting::QuotaSelectedTabId(Some(Cursor))).unwrap();
    settle().await;
    assert!(f.net.take().is_empty());
    advance(61).await;
    commands::update_setting(f.app.clone(), Setting::ShowTokensInTray(true)).unwrap();
    settle().await;
    assert_eq!(f.net.take(), [Call::Quotas(vec![Cursor])]);
    let saved = f.reload().settings.into_inner().unwrap();
    assert!(saved.show_tokens_in_tray);
    assert_eq!(saved.quota_product_order, [Grok, Cursor]);
    assert_eq!(saved.quota_selected_tab_id, Some(Cursor));
    cursor_login::after_change(
        &f.app,
        LoginStatus {
            state: LoginState::SignedOut,
            email: None,
            error: None,
            supported: Some(true),
        },
    );
    settle().await;
    assert_eq!(f.net.take(), [Call::Quotas(vec![Cursor])]);
    commands::set_quota_product_selected(f.app.clone(), Grok, true)
        .await
        .unwrap();
    assert_eq!(f.net.take(), [Call::Quotas(vec![Cursor, Grok])]);
    commands::get_rate_limits(f.app.clone(), true).await;
    assert_eq!(f.net.take(), [Call::Quotas(vec![Cursor, Grok])]);
}

#[tokio::test(start_paused = true)]
async fn preloading_panels_about_history_and_settings_snapshots_are_offline() {
    let f = Fixture::new(true, false, vec![]);
    settle().await;
    let updates: Vec<_> = updater::available()
        .then_some(Call::Update)
        .into_iter()
        .collect();
    assert_eq!(f.net.take(), updates);
    assert!(f.app.ctx().launch_network_ready());
    commands::get_cached_rate_limits(f.app.clone());
    f.app.cached_usage();
    commands::get_settings(f.app.clone());
    commands::get_update_info(f.app.clone());
    commands::get_sync_history(f.app.clone());
    commands::get_sync_state(f.app.clone());
    advance(86399).await;
    assert!(f.net.take().is_empty());
    advance(1).await;
    assert_eq!(f.net.take(), updates);
    assert_eq!(
        commands::check_for_update(f.app.clone()).await.is_ok(),
        updater::available()
    );
    assert_eq!(f.net.take(), updates);
    f.app.settings_shown();
    settle().await;
    assert_eq!(f.net.take(), [Call::Support]);
    assert_eq!(
        commands::collect_diagnostics(f.app.clone()).await,
        "synthetic report"
    );
    assert_eq!(f.net.take(), [Call::Diagnostics]);
}
