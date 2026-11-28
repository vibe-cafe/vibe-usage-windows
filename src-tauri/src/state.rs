//! Shared app state (counterpart of AppState.swift's service wiring).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;
use vibe_core::config::ConfigManager;
use vibe_core::quota_product::{normalize_selection, ZCodeQuotaRegion};
use vibe_core::rate_limit::claude_models::LiveModelHistory;
use vibe_core::{ProviderRateLimit, RateLimitProvider};

pub const IS_DEV: bool = cfg!(debug_assertions);

/// Persisted app settings — counterpart of the macOS UserDefaults keys
/// (showCostInMenuBar / showTokensInMenuBar / codexRateLimitEnabled /
/// claudeRateLimitEnabled).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub show_cost_in_tray: bool,
    pub show_tokens_in_tray: bool,
    pub codex_rate_limit_enabled: bool,
    pub claude_rate_limit_enabled: bool,
    pub selected_quota_product_ids: Vec<RateLimitProvider>,
    pub quota_selection_initialized: bool,
    pub z_code_quota_region: ZCodeQuotaRegion,
    /// 「没检测到代理时」 (`egress_check::EgressMode`), default 「自动判断」.
    pub egress_mode: crate::egress_check::EgressMode,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_cost_in_tray: true,
            show_tokens_in_tray: false,
            codex_rate_limit_enabled: true,
            claude_rate_limit_enabled: false,
            selected_quota_product_ids: Vec::new(),
            quota_selection_initialized: false,
            z_code_quota_region: ZCodeQuotaRegion::default(),
            egress_mode: crate::egress_check::EgressMode::default(),
        }
    }
}

impl AppSettings {
    pub fn egress(&self) -> crate::egress_check::Settings {
        self.egress_mode.settings()
    }

    pub fn set_quota_selection(&mut self, selection: Vec<RateLimitProvider>) {
        self.selected_quota_product_ids = normalize_selection(selection);
        self.quota_selection_initialized = true;
        // Preserve downgrade behavior for the original two toggles.
        self.codex_rate_limit_enabled = self
            .selected_quota_product_ids
            .contains(&RateLimitProvider::Codex);
        self.claude_rate_limit_enabled = self
            .selected_quota_product_ids
            .contains(&RateLimitProvider::ClaudeCode);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub status: SyncStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// epoch millis of last successful sync
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<u64>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            status: SyncStatus::Idle,
            message: None,
            last_sync_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Default)]
pub struct RateLimitCache {
    pub refreshing: bool,
    pub snapshots: HashMap<RateLimitProvider, (ProviderRateLimit, Instant)>,
    last_claude_models: Option<LiveModelHistory>,
    claude_generation: u64,
    zcode_generation: u64,
    cursor_generation: u64,
}

/// Discard a read that started before Claude was disabled, the ZCode
/// region/key changed or the Cursor sign-in changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheGeneration {
    claude: u64,
    zcode: u64,
    cursor: u64,
}

impl RateLimitCache {
    pub fn generation(&self) -> CacheGeneration {
        CacheGeneration {
            claude: self.claude_generation,
            zcode: self.zcode_generation,
            cursor: self.cursor_generation,
        }
    }

    /// Turning Claude off also retires observations that could fill a missing model list.
    pub fn invalidate_claude(&mut self) {
        self.claude_generation += 1;
        self.last_claude_models = None;
        self.snapshots.remove(&RateLimitProvider::ClaudeCode);
    }

    /// Called while holding settings, before exposing a changed region/key.
    pub fn invalidate_zcode(&mut self) {
        self.zcode_generation += 1;
        self.snapshots.remove(&RateLimitProvider::ZCode);
    }

    /// The Cursor sign-in changed (signed in, out, or ended): drop the card,
    /// and an answer still in flight for the previous sign-in.
    pub fn invalidate_cursor(&mut self) {
        self.cursor_generation += 1;
        self.snapshots.remove(&RateLimitProvider::Cursor);
    }

    pub fn store(&mut self, mut snapshot: ProviderRateLimit, generation: CacheGeneration) {
        let stale = match snapshot.provider {
            RateLimitProvider::ClaudeCode => generation.claude != self.claude_generation,
            RateLimitProvider::ZCode => generation.zcode != self.zcode_generation,
            RateLimitProvider::Cursor => generation.cursor != self.cursor_generation,
            _ => false,
        };
        if stale {
            return;
        }
        if snapshot.provider == RateLimitProvider::ClaudeCode {
            LiveModelHistory::reconcile(
                &mut self.last_claude_models,
                &mut snapshot,
                vibe_core::rate_limit::now_epoch(),
            );
        }
        self.snapshots
            .insert(snapshot.provider, (snapshot, Instant::now()));
    }
}

pub struct AppCtx {
    pub config: ConfigManager,
    pub http: reqwest::Client,
    pub settings_path: PathBuf,
    pub settings: Mutex<AppSettings>,
    pub sync_state: Mutex<SyncState>,
    /// Mutual exclusion for the CLI subprocess (同步已在进行中 guard).
    pub sync_running: tokio::sync::Mutex<()>,
    /// Set when a sync is requested while one is already running so the
    /// in-flight run drains a follow-up instead of dropping the request.
    pub sync_pending: AtomicBool,
    /// Whether the most recent sync request came from the user (立即同步 /
    /// after a settings change) rather than the scheduler; recorded in the
    /// app's own sync history.
    pub sync_requested_manually: AtomicBool,
    pub device_link_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub scheduler_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// Prevent overlapping UI events from launching duplicate network/probe work.
    pub rate_limit_refresh: tokio::sync::Mutex<()>,
    pub rate_limits: Mutex<RateLimitCache>,
    pub update_info: Mutex<Option<UpdateInfo>>,
    /// (cost, tokens) for the active time range, pushed by the frontend.
    pub tray_stats: Mutex<Option<(f64, i64)>>,
    /// False while a login launch (`--autostart`) waits for the user's proxy
    /// (`system_proxy::wait_for_launch_proxy`): quota reads and syncs wait for
    /// it (`wait_launch_network`). True otherwise.
    pub launch_network: tokio::sync::watch::Sender<bool>,
    /// The Cursor sign-in this app keeps (`services::cursor_login`).
    pub cursor_login: crate::services::cursor_login::CursorLogin,
    /// Where the settings window should scroll when it next shows (the
    /// Cursor card's sign-in prompt asks for its row), taken once.
    pub settings_focus: Mutex<Option<String>>,
}

impl AppCtx {
    pub fn new(app_config_dir: PathBuf) -> Self {
        let settings_path = app_config_dir.join("settings.json");
        let raw_settings = std::fs::read_to_string(&settings_path).ok();
        let raw_object = raw_settings
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|value| value.as_object().cloned());
        let had_legacy_selection = raw_object.as_ref().is_some_and(|object| {
            object.contains_key("codexRateLimitEnabled")
                || object.contains_key("claudeRateLimitEnabled")
        });
        let mut settings: AppSettings = raw_settings
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();
        // The two egress switches of development builds become the menu's
        // option; the next save writes `egressMode` only.
        if let Some(mode) = raw_object.as_ref().and_then(crate::egress_check::EgressMode::from_legacy) {
            settings.egress_mode = mode;
        }
        let migrated_legacy_selection =
            !settings.quota_selection_initialized && had_legacy_selection;
        if migrated_legacy_selection {
            let selection = [
                settings
                    .codex_rate_limit_enabled
                    .then_some(RateLimitProvider::Codex),
                settings
                    .claude_rate_limit_enabled
                    .then_some(RateLimitProvider::ClaudeCode),
            ]
            .into_iter()
            .flatten()
            .collect();
            settings.set_quota_selection(selection);
            if let Ok(data) = serde_json::to_string_pretty(&settings) {
                let _ = vibe_core::config::atomic_write(&settings_path, data.as_bytes());
            }
        }
        Self {
            config: ConfigManager::new(IS_DEV),
            http: reqwest::Client::builder()
                .user_agent(format!("VibeUsageWindows/{}", env!("CARGO_PKG_VERSION")))
                // The proxy is resolved for every request (`system_proxy`):
                // the app follows the user's current settings, PAC included,
                // instead of the ones in place when it started.
                .proxy(reqwest::Proxy::custom(|url| {
                    crate::system_proxy::live_route(url).reqwest_proxy()
                }))
                // No pooled connections: a connection opened before the user
                // switched the proxy on must not carry the next request.
                .pool_max_idle_per_host(0)
                // A hung connect must fail fast instead of pinning spinners.
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("http client"),
            settings_path,
            settings: Mutex::new(settings),
            sync_state: Mutex::new(SyncState::default()),
            sync_running: tokio::sync::Mutex::new(()),
            sync_pending: AtomicBool::new(false),
            sync_requested_manually: AtomicBool::new(false),
            device_link_task: Mutex::new(None),
            scheduler_task: Mutex::new(None),
            rate_limit_refresh: tokio::sync::Mutex::new(()),
            rate_limits: Mutex::new(RateLimitCache::default()),
            update_info: Mutex::new(None),
            tray_stats: Mutex::new(None),
            launch_network: tokio::sync::watch::channel(true).0,
            cursor_login: Default::default(),
            settings_focus: Mutex::new(None),
        }
    }

    /// Returns once a login launch's proxy is up or its wait ran out; at once
    /// for any other launch.
    /// Whether a login launch's wait for the proxy is over (always true for
    /// any other start).
    pub fn launch_network_ready(&self) -> bool {
        *self.launch_network.borrow()
    }

    pub async fn wait_launch_network(&self) {
        let mut ready = self.launch_network.subscribe();
        let _ = ready.wait_for(|ready| *ready).await;
    }

    pub fn save_settings(&self) {
        let settings = self.settings.lock().unwrap().clone();
        if let Ok(data) = serde_json::to_string_pretty(&settings) {
            let _ = vibe_core::config::atomic_write(&self.settings_path, data.as_bytes());
        }
    }

    pub fn hostname() -> Option<String> {
        let raw = gethostname::gethostname().to_string_lossy().to_string();
        let cleaned = raw.trim().trim_end_matches(".local").to_string();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn turning_claude_off_discards_the_in_flight_read_and_last_live_models() {
        use vibe_core::rate_limit::claude_models::AccountScope;
        use vibe_core::{RateLimitSource, RateLimitStatus};
        let mut cache = RateLimitCache::default();
        let before = cache.generation();
        let mut live = ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Ok);
        live.source = Some(RateLimitSource::Live);
        live.claude_account = AccountScope::new(Some("a@example.invalid"), None);
        live.model_scoped_meters = Some(vec![]);
        cache.store(live.clone(), before);
        assert!(cache.last_claude_models.is_some());
        cache.invalidate_claude();
        cache.store(live, before);
        assert!(cache.last_claude_models.is_none());
        assert!(!cache.snapshots.contains_key(&RateLimitProvider::ClaudeCode));
    }

    #[test]
    fn region_or_key_change_discards_cached_and_in_flight_zcode_results() {
        use vibe_core::RateLimitStatus;
        let mut cache = RateLimitCache::default();
        let old_request = cache.generation();
        let mut old_account =
            ProviderRateLimit::empty(RateLimitProvider::ZCode, RateLimitStatus::Ok);
        old_account.plan_label = Some("old account".into());
        cache.store(old_account.clone(), old_request);
        cache.invalidate_zcode();
        assert!(!cache.snapshots.contains_key(&RateLimitProvider::ZCode));
        cache.store(old_account, old_request);
        assert!(!cache.snapshots.contains_key(&RateLimitProvider::ZCode));
        cache.store(
            ProviderRateLimit::empty(RateLimitProvider::Grok, RateLimitStatus::NoData),
            old_request,
        );
        assert!(cache.snapshots.contains_key(&RateLimitProvider::Grok));
        let current_request = cache.generation();
        cache.store(
            ProviderRateLimit::empty(RateLimitProvider::ZCode, RateLimitStatus::RetryableError),
            current_request,
        );
        assert_eq!(
            cache.snapshots[&RateLimitProvider::ZCode].0.status,
            RateLimitStatus::RetryableError
        );
    }

    #[test]
    fn a_cursor_answer_for_the_previous_sign_in_is_not_kept() {
        use vibe_core::RateLimitStatus;
        let mut cache = RateLimitCache::default();
        let before = cache.generation();
        let mut previous = ProviderRateLimit::empty(RateLimitProvider::Cursor, RateLimitStatus::Ok);
        previous.plan_label = Some("previous account".into());
        cache.store(previous.clone(), before);
        // Signed out (or in as someone else) while a read was in flight.
        cache.invalidate_cursor();
        assert!(!cache.snapshots.contains_key(&RateLimitProvider::Cursor));
        cache.store(previous, before);
        assert!(!cache.snapshots.contains_key(&RateLimitProvider::Cursor));
        // Other products of that refresh are kept; a read that started after it counts.
        cache.store(ProviderRateLimit::empty(RateLimitProvider::Grok, RateLimitStatus::NoData), before);
        assert!(cache.snapshots.contains_key(&RateLimitProvider::Grok));
        cache.store(ProviderRateLimit::empty(RateLimitProvider::Cursor, RateLimitStatus::NoData), cache.generation());
        assert!(cache.snapshots.contains_key(&RateLimitProvider::Cursor));
    }

    #[test]
    fn refresh_failure_replaces_old_success_instead_of_reviving_expired_meters() {
        use vibe_core::RateLimitStatus;
        let mut cache = RateLimitCache::default();
        for provider in [
            RateLimitProvider::KimiCode,
            RateLimitProvider::ZCode,
            RateLimitProvider::Grok,
        ] {
            cache.store(ProviderRateLimit::empty(provider, RateLimitStatus::Ok), CacheGeneration::default());
            cache.store(
                ProviderRateLimit::empty(provider, RateLimitStatus::RetryableError),
                CacheGeneration::default(),
            );
            assert_eq!(
                cache.snapshots[&provider].0.status,
                RateLimitStatus::RetryableError
            );
        }
    }

    #[test]
    fn migrates_legacy_provider_toggles_once() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("settings.json"),
            r#"{"codexRateLimitEnabled":false,"claudeRateLimitEnabled":true}"#,
        )
        .unwrap();

        let ctx = AppCtx::new(directory.path().to_path_buf());
        let settings = ctx.settings.lock().unwrap().clone();
        assert!(settings.quota_selection_initialized);
        assert_eq!(
            settings.selected_quota_product_ids,
            vec![RateLimitProvider::ClaudeCode]
        );
    }

    #[test]
    fn preserves_an_intentionally_empty_new_selection() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("settings.json"),
            r#"{"selectedQuotaProductIds":[],"quotaSelectionInitialized":true}"#,
        )
        .unwrap();

        let ctx = AppCtx::new(directory.path().to_path_buf());
        let settings = ctx.settings.lock().unwrap().clone();
        assert!(settings.quota_selection_initialized);
        assert!(settings.selected_quota_product_ids.is_empty());
    }
}
