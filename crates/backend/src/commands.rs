//! Typed commands for the native host.

use crate::services::api_client::UsageQuery;
use crate::services::{
    auto_launch, cursor_login, device_link, rate_limits, scheduler, sync_engine, test_diagnostics,
    updater, zcode_credentials,
};
use crate::state::{AppCtx, AppSettings, SyncState, UpdateInfo};
use crate::Backend;
use serde::Serialize;
use serde_json::Value;
use vibe_core::quota_product::{self, QuotaProduct, ZCodeQuotaRegion};
use vibe_core::{ProviderRateLimit, RateLimitProvider};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    configured: bool,
    hostname: Option<String>,
    api_url: String,
    version: String,
    is_dev: bool,
    runtime_available: bool,
    test_diagnostics_available: bool,
    updates_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key_display: Option<String>,
}

pub fn get_app_status(app: Backend) -> AppStatus {
    let ctx = app.ctx();
    let config = ctx.config.load().unwrap_or_default();
    let api_url = ctx.config.effective_api_url(&config);
    let api_key = config.api_key;

    let api_key_display = api_key.as_ref().map(|key| {
        if key.chars().count() > 12 {
            let prefix: String = key.chars().take(8).collect();
            let suffix: String = key
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("{prefix}...{suffix}")
        } else {
            key.clone()
        }
    });

    AppStatus {
        configured: api_key.is_some(),
        hostname: config.hostname.or_else(AppCtx::hostname),
        api_url,
        version: app.options().version.clone(),
        is_dev: crate::state::IS_DEV,
        runtime_available: sync_engine::detect_runtime(&app).is_some(),
        test_diagnostics_available: test_diagnostics::available(),
        updates_available: updater::available(),
        api_key_display,
    }
}

pub async fn fetch_usage(app: Backend, query: UsageQuery) -> Result<Value, String> {
    app.select_usage(query).await
}

// -- Device link --------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLinkStart {
    user_code: String,
}

#[cfg(feature = "ffi-fixtures")]
pub mod wire_fixtures;

pub async fn start_device_link(app: Backend) -> Result<DeviceLinkStart, String> {
    device_link::start(app)
        .await
        .map(|user_code| DeviceLinkStart { user_code })
}

pub fn cancel_device_link(app: Backend) {
    device_link::cancel(&app);
}

// -- Sync ---------------------------------------------------------------------

pub fn trigger_sync(app: Backend) {
    let worker = app.clone();
    app.spawn(async move {
        worker
            .sync(crate::services::sync_history::SyncTrigger::Manual)
            .await;
    });
}

/// 「复制诊断信息」: whitelisted, typed fields only (see `services::diagnostics`).
pub async fn collect_diagnostics(app: Backend) -> String {
    app.ctx().wait_launch_network().await;
    app.0.network.diagnostics(&app).await
}

pub fn get_sync_state(app: Backend) -> SyncState {
    app.ctx().sync_state.lock().unwrap().clone()
}

// -- Rate limits ----------------------------------------------------------------

pub async fn get_rate_limits(app: Backend, force: bool) -> Vec<ProviderRateLimit> {
    // During a login launch's wait for the proxy this answers once it is up
    // (`rate_limits::get_rate_limits`); the window shows its loading state.
    app.quotas(force).await
}

pub fn get_cached_rate_limits(app: Backend) -> rate_limits::CachedRateLimits {
    rate_limits::cached_rate_limits(&app)
}

fn initial_quota_selection(
    products: &[QuotaProduct],
    zcode_configured: bool,
) -> Vec<RateLimitProvider> {
    let eligible = products
        .iter()
        .filter(|product| product.provider != RateLimitProvider::ZCode || zcode_configured)
        // Antigravity is never turned on for the user (maintainer 2026-11-26):
        // Google does not support third-party quota reads on personal plans, so
        // reading it is left to the user's own switch. This runs only before the
        // selection is initialized; an existing selection is kept as it is.
        .filter(|product| product.provider != RateLimitProvider::Antigravity)
        .cloned()
        .collect::<Vec<_>>();
    quota_product::initial_selection(&eligible)
}

pub fn get_quota_products(app: Backend) -> Vec<QuotaProduct> {
    let products = quota_product::discover();
    test_diagnostics::record_discovery(
        &app.options().local_data,
        products
            .iter()
            .filter(|product| product.is_detected)
            .map(|product| product.provider)
            .collect(),
    );
    products
}

/// Run once before the backend is exposed to its host. Discovery commands
/// subsequently remain read-only, including concurrent panel/settings reads.
pub(crate) fn initialize_quota_selection(ctx: &AppCtx, local_data: &std::path::Path) {
    let mut settings = ctx.settings.lock().unwrap();
    if !settings.quota_selection_initialized {
        let products = quota_product::discover();
        let region = settings.z_code_quota_region;
        let zcode_configured = zcode_credentials::load(region).ok().flatten().is_some();
        // A detected ZCode installation cannot yield quota without a key the
        // user explicitly gave this app, so it must not consume a default slot.
        let initial = initial_quota_selection(&products, zcode_configured);
        settings.set_quota_selection(initial);
        let selected = settings.selected_quota_product_ids.clone();
        drop(settings);
        ctx.save_settings();
        test_diagnostics::record_selection_initialized(local_data, selected);
    }
}

pub async fn set_quota_product_selected(
    app: Backend,
    provider: RateLimitProvider,
    selected: bool,
) -> Result<Vec<ProviderRateLimit>, String> {
    {
        let ctx = app.ctx();
        let mut settings = ctx.settings.lock().unwrap();
        let next = quota_product::update_selection(
            &settings.selected_quota_product_ids,
            provider,
            selected,
        );
        settings.set_quota_selection(next);
        if provider == RateLimitProvider::ClaudeCode && !selected {
            ctx.rate_limits.lock().unwrap().invalidate_claude();
        }
        drop(settings);
        ctx.save_settings();
        let settings = ctx.settings.lock().unwrap().clone();
        test_diagnostics::record_selection_changed(
            &app.options().local_data,
            settings.selected_quota_product_ids.clone(),
        );
        let _ = app.emit("settings-updated", &settings);
    }
    Ok(rate_limits::after_settings_change(&app).await)
}

pub fn get_zcode_credential_status() -> Result<zcode_credentials::ZCodeCredentialStatus, String> {
    zcode_credentials::status()
}

pub async fn set_zcode_quota_region(
    app: Backend,
    region: ZCodeQuotaRegion,
) -> Result<Vec<ProviderRateLimit>, String> {
    let configured = zcode_credentials::load(region)?.is_some();
    {
        let ctx = app.ctx();
        let mut settings = ctx.settings.lock().unwrap();
        let previous_selection = settings.selected_quota_product_ids.clone();
        if settings.z_code_quota_region != region {
            ctx.rate_limits.lock().unwrap().invalidate_zcode();
        }
        settings.z_code_quota_region = region;
        if !configured
            && settings
                .selected_quota_product_ids
                .contains(&RateLimitProvider::ZCode)
        {
            let next = quota_product::update_selection(
                &settings.selected_quota_product_ids,
                RateLimitProvider::ZCode,
                false,
            );
            settings.set_quota_selection(next);
        }
        drop(settings);
        ctx.save_settings();
        let settings = ctx.settings.lock().unwrap().clone();
        if settings.selected_quota_product_ids != previous_selection {
            test_diagnostics::record_selection_changed(
                &app.options().local_data,
                settings.selected_quota_product_ids.clone(),
            );
        }
        let _ = app.emit("settings-updated", &settings);
    }
    Ok(rate_limits::after_settings_change(&app).await)
}

pub async fn set_zcode_api_key(
    app: Backend,
    region: ZCodeQuotaRegion,
    api_key: Option<String>,
) -> Result<Vec<ProviderRateLimit>, String> {
    let configured = api_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    {
        let ctx = app.ctx();
        let mut settings = ctx.settings.lock().unwrap();
        zcode_credentials::store(region, api_key.as_deref())?;
        ctx.rate_limits.lock().unwrap().invalidate_zcode();
        let previous_selection = settings.selected_quota_product_ids.clone();
        settings.z_code_quota_region = region;
        if !configured
            && settings
                .selected_quota_product_ids
                .contains(&RateLimitProvider::ZCode)
        {
            let next = quota_product::update_selection(
                &settings.selected_quota_product_ids,
                RateLimitProvider::ZCode,
                false,
            );
            settings.set_quota_selection(next);
        }
        drop(settings);
        ctx.save_settings();
        let settings = ctx.settings.lock().unwrap().clone();
        if settings.selected_quota_product_ids != previous_selection {
            test_diagnostics::record_selection_changed(
                &app.options().local_data,
                settings.selected_quota_product_ids.clone(),
            );
        }
        let _ = app.emit("settings-updated", &settings);
    }
    Ok(rate_limits::after_settings_change(&app).await)
}

// -- Cursor sign-in (`services::cursor_login`) ------------------------------------

/// Read-only snapshot. Only Backend::settings_shown schedules the CLI check.
pub fn get_cursor_login(app: Backend) -> cursor_login::LoginStatus {
    cursor_login::status(&app)
}

pub fn start_cursor_login(app: Backend) -> cursor_login::LoginStatus {
    cursor_login::start(&app)
}

pub fn cancel_cursor_login(app: Backend) -> cursor_login::LoginStatus {
    cursor_login::cancel(&app)
}

pub fn logout_cursor(app: Backend) -> Result<cursor_login::LoginStatus, String> {
    cursor_login::logout(&app)
}

// -- Settings -------------------------------------------------------------------

pub fn get_settings(app: Backend) -> AppSettings {
    app.ctx().settings.lock().unwrap().clone()
}

/// Exactly one independently editable preference per command. Selection,
/// credentials and region have dedicated commands with their own invalidation.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Setting {
    ShowCostInTray(bool),
    ShowTokensInTray(bool),
    EgressMode(crate::EgressMode),
    QuotaProductOrder(Vec<RateLimitProvider>),
    QuotaSelectedTabId(Option<RateLimitProvider>),
}

pub fn update_setting(app: Backend, change: Setting) -> Result<AppSettings, String> {
    let mut egress_changed = false;
    let snapshot = app.ctx().update_settings(|next| match change {
        Setting::ShowCostInTray(value) => next.show_cost_in_tray = value,
        Setting::ShowTokensInTray(value) => next.show_tokens_in_tray = value,
        Setting::EgressMode(value) => {
            egress_changed = next.egress_mode != value;
            next.egress_mode = value;
        }
        Setting::QuotaProductOrder(order) => {
            next.quota_product_order = quota_product::normalize_selection(order)
        }
        Setting::QuotaSelectedTabId(provider) => next.quota_selected_tab_id = provider,
    })?;
    let _ = app.emit("settings-updated", &snapshot);
    if egress_changed {
        app.refresh_quotas();
    }
    Ok(snapshot)
}

pub fn get_sync_history(app: Backend) -> Vec<crate::SyncRecord> {
    let file = crate::services::sync_history::path_in(&app.options().roaming_config);
    crate::services::sync_history::load(&file)
}

pub fn get_launch_at_login() -> Result<bool, String> {
    auto_launch::get()
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    auto_launch::set(enabled)
}

fn validate_extra_root_source(source: &str) -> Result<(), String> {
    match source {
        "codex" | "grok" | "antigravity" => Ok(()),
        _ => Err("不支持的数据源".into()),
    }
}

pub async fn get_extra_roots(app: Backend) -> Result<Value, String> {
    let output = sync_engine::run_config_command(&app, &["config", "roots"]).await?;
    serde_json::from_str(&output).map_err(|e| format!("无法读取隔离运行时目录: {e}"))
}

pub async fn add_extra_root(app: Backend, source: String, path: String) -> Result<(), String> {
    validate_extra_root_source(&source)?;
    sync_engine::run_config_command(&app, &["config", "add-root", &source, &path]).await?;
    Ok(())
}

pub async fn remove_extra_root(app: Backend, source: String, path: String) -> Result<(), String> {
    validate_extra_root_source(&source)?;
    sync_engine::run_config_command(&app, &["config", "remove-root", &source, &path]).await?;
    Ok(())
}

pub fn reset_config(app: Backend) -> Result<(), String> {
    device_link::cancel(&app);
    let mut generation = app.ctx().account_generation.lock().unwrap();
    *generation += 1;
    scheduler::stop(&app);
    let ctx = app.ctx();
    ctx.config.reset().map_err(|e| e.to_string())?;
    // The app's own sync history goes with the configuration it describes.
    if let Some(dir) = ctx.settings_path.parent() {
        let _ = std::fs::remove_file(crate::services::sync_history::path_in(dir));
    }
    app.reset_sync_state()?;
    drop(generation);
    let _ = app.emit("sync-state", get_sync_state(app.clone()));
    Ok(())
}

// -- Shell / windows ---------------------------------------------------------------

pub fn open_external(url: String) -> Result<(), String> {
    crate::process_utils::shell_open(&url)
}

pub fn export_test_diagnostics(app: Backend, destination: String) -> Result<(), String> {
    let destination = destination.trim();
    if destination.is_empty() {
        return Err("请选择导出位置".into());
    }
    test_diagnostics::export(&app.options().local_data, std::path::Path::new(destination))
}

// -- Updates -----------------------------------------------------------------------

pub async fn check_for_update(app: Backend) -> Result<Option<UpdateInfo>, String> {
    updater::require_updates()?;
    app.0.network.update(&app).await
}

/// The update the latest check found, if any. The check at startup can finish
/// before a window has subscribed to `update-available`, so each window reads
/// this once after subscribing. Read-only: it never starts a check.
pub fn get_update_info(app: Backend) -> Option<UpdateInfo> {
    app.ctx().update_info.lock().unwrap().clone()
}

pub async fn install_update(app: Backend) -> Result<(), String> {
    updater::install(&app).await
}

#[cfg(test)]
mod tests {
    use super::{initial_quota_selection, validate_extra_root_source};
    use vibe_core::quota_product::{QuotaProduct, QuotaProductAvailability};
    use vibe_core::RateLimitProvider;

    #[test]
    fn startup_preserves_explicit_empty_selection_and_does_not_reinitialize() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = crate::state::AppCtx::new(directory.path().to_path_buf());
        ctx.settings.lock().unwrap().set_quota_selection(Vec::new());
        super::initialize_quota_selection(&ctx, directory.path());
        assert!(ctx
            .settings
            .lock()
            .unwrap()
            .selected_quota_product_ids
            .is_empty());
        assert!(!directory.path().join("settings.json").exists());
    }

    #[test]
    fn extra_root_sources_are_allowlisted() {
        assert!(validate_extra_root_source("codex").is_ok());
        assert!(validate_extra_root_source("grok").is_ok());
        assert!(validate_extra_root_source("antigravity").is_ok());
        assert!(validate_extra_root_source("cursor").is_err());
    }

    #[test]
    fn unconfigured_zcode_does_not_block_a_later_default_slot() {
        let ready = |provider| QuotaProduct {
            provider,
            display_name: "fixture",
            availability: QuotaProductAvailability::Ready,
            is_detected: true,
        };
        let products = vec![
            ready(RateLimitProvider::KimiCode),
            ready(RateLimitProvider::ZCode),
            ready(RateLimitProvider::Grok),
        ];
        assert_eq!(
            initial_quota_selection(&products, false),
            vec![RateLimitProvider::KimiCode, RateLimitProvider::Grok]
        );
    }

    #[test]
    fn detected_antigravity_is_not_turned_on_at_first_launch() {
        let ready = |provider| QuotaProduct {
            provider,
            display_name: "fixture",
            availability: QuotaProductAvailability::Ready,
            is_detected: true,
        };
        let products = vec![
            ready(RateLimitProvider::Codex),
            ready(RateLimitProvider::Antigravity),
            ready(RateLimitProvider::Cursor),
        ];
        assert_eq!(
            initial_quota_selection(&products, true),
            vec![RateLimitProvider::Codex, RateLimitProvider::Cursor]
        );
    }
}
