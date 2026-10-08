//! Tauri commands — the app's entire invoke surface (see src/lib/api.ts).

use crate::services::api_client::{self, UsageQuery};
use crate::services::{
    auto_launch, device_link, rate_limits, scheduler, sync_engine, test_diagnostics, updater,
    zcode_credentials,
};
use crate::state::{AppCtx, AppSettings, SyncState, UpdateInfo};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
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

#[tauri::command]
pub fn get_app_status(app: AppHandle) -> AppStatus {
    let ctx = app.state::<AppCtx>();
    let config = ctx.config.load();
    let api_key = config.as_ref().and_then(|c| c.api_key.clone());
    let api_url = config
        .as_ref()
        .and_then(|c| c.api_url.clone())
        .unwrap_or_else(|| ctx.config.default_api_url().to_string());

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
        hostname: config.as_ref().and_then(|c| c.hostname.clone()).or_else(AppCtx::hostname),
        api_url,
        version: app.package_info().version.to_string(),
        is_dev: crate::state::IS_DEV,
        runtime_available: sync_engine::detect_runtime(&app).is_some(),
        test_diagnostics_available: test_diagnostics::available(),
        updates_available: updater::available(),
        api_key_display,
    }
}

#[tauri::command]
pub async fn fetch_usage(app: AppHandle, query: UsageQuery) -> Result<Value, String> {
    let (http, base_url, api_key) = {
        let ctx = app.state::<AppCtx>();
        let config = ctx.config.load().ok_or("未配置")?;
        let api_key = config.api_key.ok_or("未配置")?;
        let base_url = config
            .api_url
            .unwrap_or_else(|| ctx.config.default_api_url().to_string());
        (ctx.http.clone(), base_url, api_key)
    };
    api_client::fetch_usage(&http, &base_url, &api_key, &query)
        .await
        .map_err(|e| e.to_string())
}

// -- Device link --------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLinkStart {
    user_code: String,
}

#[tauri::command]
pub async fn start_device_link(app: AppHandle) -> Result<DeviceLinkStart, String> {
    device_link::start(app)
        .await
        .map(|user_code| DeviceLinkStart { user_code })
}

#[tauri::command]
pub fn cancel_device_link(app: AppHandle) {
    device_link::cancel(&app);
}

/// CI / no-browser fallback — validates the pre-issued key with a live
/// `GET /api/usage?days=1` before saving (mirrors CLI --manual-key intent).
#[tauri::command]
pub async fn set_manual_key(app: AppHandle, api_key: String) -> Result<(), String> {
    let api_key = api_key.trim().to_string();
    if !api_key.starts_with("vbu_") {
        return Err("API Key 必须以 vbu_ 开头".into());
    }
    let (http, base_url) = {
        let ctx = app.state::<AppCtx>();
        (ctx.http.clone(), ctx.config.default_api_url().to_string())
    };
    api_client::fetch_usage(&http, &base_url, &api_key, &UsageQuery::Days { days: 1 })
        .await
        .map_err(|e| e.to_string())?;
    device_link::configure(&app, api_key, base_url);
    Ok(())
}

// -- Sync ---------------------------------------------------------------------

#[tauri::command]
pub fn trigger_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        sync_engine::run_sync(app).await;
    });
}

#[tauri::command]
pub fn get_sync_state(app: AppHandle) -> SyncState {
    app.state::<AppCtx>().sync_state.lock().unwrap().clone()
}

// -- Rate limits ----------------------------------------------------------------

#[tauri::command]
pub async fn get_rate_limits(app: AppHandle, force: bool) -> Vec<ProviderRateLimit> {
    rate_limits::get_rate_limits(&app, force).await
}

#[tauri::command]
pub async fn enable_claude_rate_limit(app: AppHandle) -> Result<Vec<ProviderRateLimit>, String> {
    rate_limits::enable_claude(&app).await
}

fn initial_quota_selection(
    products: &[QuotaProduct],
    zcode_configured: bool,
) -> Vec<RateLimitProvider> {
    let eligible = products
        .iter()
        .filter(|product| product.provider != RateLimitProvider::ZCode || zcode_configured)
        .cloned()
        .collect::<Vec<_>>();
    quota_product::initial_selection(&eligible)
}

#[tauri::command]
pub fn get_quota_products() -> Vec<QuotaProduct> {
    let products = quota_product::discover();
    test_diagnostics::record_discovery(
        products
            .iter()
            .filter(|product| product.is_detected)
            .map(|product| product.provider)
            .collect(),
    );
    products
}

/// Run once before the Tauri context is exposed to windows. Discovery commands
/// subsequently remain read-only, including concurrent panel/settings reads.
pub fn initialize_quota_selection(ctx: &AppCtx) {
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
        test_diagnostics::record_selection_initialized(selected);
    }
}

#[tauri::command]
pub async fn set_quota_product_selected(
    app: AppHandle,
    provider: RateLimitProvider,
    selected: bool,
) -> Result<Vec<ProviderRateLimit>, String> {
    {
        let ctx = app.state::<AppCtx>();
        let mut settings = ctx.settings.lock().unwrap();
        let next = quota_product::update_selection(
            &settings.selected_quota_product_ids,
            provider,
            selected,
        );
        settings.set_quota_selection(next);
        drop(settings);
        ctx.save_settings();
        let settings = ctx.settings.lock().unwrap().clone();
        test_diagnostics::record_selection_changed(settings.selected_quota_product_ids.clone());
        let _ = app.emit("settings-updated", &settings);
    }
    Ok(rate_limits::get_rate_limits(&app, true).await)
}

#[tauri::command]
pub fn get_zcode_credential_status() -> Result<zcode_credentials::ZCodeCredentialStatus, String> {
    zcode_credentials::status()
}

#[tauri::command]
pub async fn set_zcode_quota_region(
    app: AppHandle,
    region: ZCodeQuotaRegion,
) -> Result<Vec<ProviderRateLimit>, String> {
    let configured = zcode_credentials::load(region)?.is_some();
    {
        let ctx = app.state::<AppCtx>();
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
            test_diagnostics::record_selection_changed(settings.selected_quota_product_ids.clone());
        }
        let _ = app.emit("settings-updated", &settings);
    }
    Ok(rate_limits::get_rate_limits(&app, true).await)
}

#[tauri::command]
pub async fn set_zcode_api_key(
    app: AppHandle,
    region: ZCodeQuotaRegion,
    api_key: Option<String>,
) -> Result<Vec<ProviderRateLimit>, String> {
    let configured = api_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    {
        let ctx = app.state::<AppCtx>();
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
            test_diagnostics::record_selection_changed(settings.selected_quota_product_ids.clone());
        }
        let _ = app.emit("settings-updated", &settings);
    }
    Ok(rate_limits::get_rate_limits(&app, true).await)
}

// -- Settings -------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    app.state::<AppCtx>().settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, mut settings: AppSettings) {
    if settings.quota_selection_initialized {
        let selection = settings.selected_quota_product_ids.clone();
        settings.set_quota_selection(selection);
    }
    {
        let ctx = app.state::<AppCtx>();
        let mut current = ctx.settings.lock().unwrap();
        if current.z_code_quota_region != settings.z_code_quota_region {
            ctx.rate_limits.lock().unwrap().invalidate_zcode();
        }
        *current = settings.clone();
    }
    let ctx = app.state::<AppCtx>();
    ctx.save_settings();

    let _ = app.emit("settings-updated", &settings);
    crate::tray::update_tray(&app);
}

#[tauri::command]
pub fn get_launch_at_login() -> Result<bool, String> {
    auto_launch::get()
}

#[tauri::command]
pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    auto_launch::set(enabled)
}

fn validate_extra_root_source(source: &str) -> Result<(), String> {
    match source {
        "codex" | "grok" | "antigravity" => Ok(()),
        _ => Err("不支持的数据源".into()),
    }
}

#[tauri::command]
pub async fn get_extra_roots(app: AppHandle) -> Result<Value, String> {
    let output = sync_engine::run_config_command(&app, &["config", "roots"]).await?;
    serde_json::from_str(&output).map_err(|e| format!("无法读取隔离运行时目录: {e}"))
}

#[tauri::command]
pub async fn add_extra_root(app: AppHandle, source: String, path: String) -> Result<(), String> {
    validate_extra_root_source(&source)?;
    sync_engine::run_config_command(&app, &["config", "add-root", &source, &path]).await?;
    Ok(())
}

#[tauri::command]
pub async fn remove_extra_root(app: AppHandle, source: String, path: String) -> Result<(), String> {
    validate_extra_root_source(&source)?;
    sync_engine::run_config_command(&app, &["config", "remove-root", &source, &path]).await?;
    Ok(())
}

#[tauri::command]
pub fn reset_config(app: AppHandle) -> Result<(), String> {
    scheduler::stop(&app);
    let ctx = app.state::<AppCtx>();
    ctx.config.reset().map_err(|e| e.to_string())?;
    *ctx.tray_stats.lock().unwrap() = None;
    crate::tray::update_tray(&app);
    Ok(())
}

// -- Shell / windows ---------------------------------------------------------------

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("仅允许打开 http(s) 链接".into());
    }
    crate::process_utils::shell_open(&url)
}

pub fn open_settings_impl(app: &AppHandle) {
    if let Some(existing) = app.get_webview_window("settings") {
        let _ = existing.show();
        let _ = existing.set_focus();
        let _ = app.emit("settings-shown", ());
        return;
    }
    // The packaged app declares a hidden settings window in tauri.conf.json so
    // WebView has loaded before the tray menu asks to show it. Keep this as a
    // recovery path in case the window was closed by the platform.
    // Keep the app URL query-free here: packaged asset loading treats the
    // whole string as an app resource path on some WebView/Tauri versions.
    let result = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
        .title("Vibe Usage 设置")
        .inner_size(460.0, 620.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
    match result {
        Ok(_) => {
            let _ = app.emit("settings-shown", ());
        }
        Err(error) => {
            log::error!("settings window: {error}");
        }
    }
}

#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    open_settings_impl(&app);
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    crate::panel::hide_now(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn export_test_diagnostics(destination: String) -> Result<(), String> {
    let destination = destination.trim();
    if destination.is_empty() {
        return Err("请选择导出位置".into());
    }
    test_diagnostics::export(std::path::Path::new(destination))
}

// -- Tray ------------------------------------------------------------------------

/// Pushed by the frontend after each fetch/range change: cost + tokens for
/// the ACTIVE time range (no filters) — mirrors menuBarCost/menuBarTokens.
#[tauri::command]
pub fn update_tray_stats(app: AppHandle, cost: f64, tokens: i64) {
    {
        let ctx = app.state::<AppCtx>();
        *ctx.tray_stats.lock().unwrap() = Some((cost, tokens));
    }
    crate::tray::update_tray(&app);
}

// -- Updates -----------------------------------------------------------------------

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    updater::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
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
        super::initialize_quota_selection(&ctx);
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
}
