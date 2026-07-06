//! Shared app state (counterpart of AppState.swift's service wiring).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use vibe_core::config::ConfigManager;
use vibe_core::ProviderRateLimit;

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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_cost_in_tray: true,
            show_tokens_in_tray: false,
            codex_rate_limit_enabled: true,
            claude_rate_limit_enabled: false,
        }
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
    pub codex: Option<(ProviderRateLimit, Instant)>,
    pub claude: Option<(ProviderRateLimit, Instant)>,
}

pub struct AppCtx {
    pub config: ConfigManager,
    pub http: reqwest::Client,
    pub settings_path: PathBuf,
    pub settings: Mutex<AppSettings>,
    pub sync_state: Mutex<SyncState>,
    /// Mutual exclusion for the CLI subprocess (同步已在进行中 guard).
    pub sync_running: tokio::sync::Mutex<()>,
    pub device_link_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub scheduler_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub rate_limits: Mutex<RateLimitCache>,
    pub update_info: Mutex<Option<UpdateInfo>>,
    /// (cost, tokens) for the active time range, pushed by the frontend.
    pub tray_stats: Mutex<Option<(f64, i64)>>,
}

impl AppCtx {
    pub fn new(app_config_dir: PathBuf) -> Self {
        let settings_path = app_config_dir.join("settings.json");
        let settings = std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            config: ConfigManager::new(IS_DEV),
            http: reqwest::Client::builder()
                .user_agent(format!("VibeUsageWindows/{}", env!("CARGO_PKG_VERSION")))
                // system-proxy feature honors the Windows proxy; a hung
                // connect must fail fast instead of pinning spinners.
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("http client"),
            settings_path,
            settings: Mutex::new(settings),
            sync_state: Mutex::new(SyncState::default()),
            sync_running: tokio::sync::Mutex::new(()),
            device_link_task: Mutex::new(None),
            scheduler_task: Mutex::new(None),
            rate_limits: Mutex::new(RateLimitCache::default()),
            update_info: Mutex::new(None),
            tray_stats: Mutex::new(None),
        }
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
