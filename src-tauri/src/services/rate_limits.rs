//! Subscription-quota coordination. Native Codex/Claude readers stay in
//! place; Kimi/ZCode/Grok use the versioned vendored-CLI contract.

use crate::services::{claude_usage, codex_usage, quota_cli, test_diagnostics};
use crate::state::AppCtx;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use vibe_core::quota_product::update_selection;
use vibe_core::rate_limit::codex;
use vibe_core::statusline_hook::StatuslineHook;
use vibe_core::{ProviderRateLimit, RateLimitProvider, RateLimitStatus};

const MAX_AGE: Duration = Duration::from_secs(60);

pub fn statusline_hook(app: &AppHandle) -> StatuslineHook {
    StatuslineHook::new(crate::services::sync_engine::node_for_statusline(app))
}

fn selected_providers(app: &AppHandle) -> Vec<RateLimitProvider> {
    let ctx = app.state::<AppCtx>();
    let settings = ctx.settings.lock().unwrap();
    if settings.quota_selection_initialized {
        return settings.selected_quota_product_ids.clone();
    }
    [
        settings
            .codex_rate_limit_enabled
            .then_some(RateLimitProvider::Codex),
        settings
            .claude_rate_limit_enabled
            .then_some(RateLimitProvider::ClaudeCode),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn needs_refresh(app: &AppHandle, provider: RateLimitProvider, force: bool) -> bool {
    let ctx = app.state::<AppCtx>();
    let cache = ctx.rate_limits.lock().unwrap();
    force
        || cache
            .snapshots
            .get(&provider)
            .map(|(_, at)| at.elapsed() > MAX_AGE)
            .unwrap_or(true)
}

fn store_snapshot(app: &AppHandle, snapshot: ProviderRateLimit, generation: u64) {
    app.state::<AppCtx>()
        .rate_limits
        .lock()
        .unwrap()
        .store(snapshot, generation);
}

pub async fn get_rate_limits(app: &AppHandle, force: bool) -> Vec<ProviderRateLimit> {
    let ctx = app.state::<AppCtx>();
    // A caller that waited for an in-flight forced refresh consumes that
    // result instead of immediately starting the same work again.
    let (_refresh_guard, effective_force) = match ctx.rate_limit_refresh.try_lock() {
        Ok(guard) => (guard, force),
        Err(_) => (ctx.rate_limit_refresh.lock().await, false),
    };
    let selected = selected_providers(app);
    // Lock order is settings -> cache, also used by credential mutation commands.
    let (region, generation) = {
        let settings = ctx.settings.lock().unwrap();
        let cache = ctx.rate_limits.lock().unwrap();
        (settings.z_code_quota_region, cache.zcode_generation())
    };
    test_diagnostics::record_refresh_started(selected.clone());

    if selected.contains(&RateLimitProvider::Codex)
        && needs_refresh(app, RateLimitProvider::Codex, effective_force)
    {
        let http = app.state::<AppCtx>().http.clone();
        let live = codex_usage::fetch(&http).await;
        let snapshot = match live {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let fallback = tauri::async_runtime::spawn_blocking(codex::read)
                    .await
                    .unwrap_or_else(|_| {
                        ProviderRateLimit::empty(RateLimitProvider::Codex, RateLimitStatus::NoData)
                    });
                if fallback.status != RateLimitStatus::NoData {
                    fallback
                } else if matches!(error, codex_usage::FetchError::Unauthorized) {
                    ProviderRateLimit::empty(
                        RateLimitProvider::Codex,
                        RateLimitStatus::Unauthorized,
                    )
                } else {
                    fallback
                }
            }
        };
        store_snapshot(app, snapshot, generation);
    }

    if selected.contains(&RateLimitProvider::ClaudeCode)
        && needs_refresh(app, RateLimitProvider::ClaudeCode, effective_force)
    {
        let snapshot = match claude_usage::fetch().await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::debug!("Claude live quota unavailable: {error}");
                tauri::async_runtime::spawn_blocking(claude_usage::cached_snapshot)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| {
                        ProviderRateLimit::empty(
                            RateLimitProvider::ClaudeCode,
                            RateLimitStatus::NoData,
                        )
                    })
            }
        };
        store_snapshot(app, snapshot, generation);
    }

    let cli_providers = selected
        .iter()
        .copied()
        .filter(|provider| quota_cli::uses_cli(*provider))
        .filter(|provider| needs_refresh(app, *provider, effective_force))
        .collect::<Vec<_>>();
    if !cli_providers.is_empty() {
        match quota_cli::fetch(app, &cli_providers, region).await {
            Ok(snapshots) => {
                for snapshot in snapshots {
                    // Only the CLI can safely fall back to its credential-scoped,
                    // expiry-checked cache. The app must not revive an old account
                    // or an expired meter after the CLI rejected that fallback.
                    store_snapshot(app, snapshot, generation);
                }
            }
            Err(error) => {
                log::debug!("CLI quota unavailable: {error}");
                test_diagnostics::record_failure(cli_providers.clone(), error.diagnostic_code());
                for provider in cli_providers {
                    store_snapshot(
                        app,
                        ProviderRateLimit::empty(provider, RateLimitStatus::RetryableError),
                        generation,
                    );
                }
            }
        }
    }

    let ctx = app.state::<AppCtx>();
    let selected = selected_providers(app);
    let cache = ctx.rate_limits.lock().unwrap();
    let result = selected
        .into_iter()
        .map(|provider| {
            cache
                .snapshots
                .get(&provider)
                .map(|(snapshot, _)| snapshot.clone())
                .unwrap_or_else(|| ProviderRateLimit::empty(provider, RateLimitStatus::NoData))
        })
        .collect::<Vec<_>>();
    drop(cache);
    for snapshot in &result {
        test_diagnostics::record_result(snapshot);
    }
    result
}

/// Compatibility command retained for older frontends. New UI uses the
/// shared product selector.
pub async fn enable_claude(app: &AppHandle) -> Result<Vec<ProviderRateLimit>, String> {
    {
        let ctx = app.state::<AppCtx>();
        let mut settings = ctx.settings.lock().unwrap();
        let next = update_selection(
            &settings.selected_quota_product_ids,
            RateLimitProvider::ClaudeCode,
            true,
        );
        settings.set_quota_selection(next);
        drop(settings);
        ctx.save_settings();
        let settings = ctx.settings.lock().unwrap().clone();
        let _ = app.emit("settings-updated", &settings);
    }
    Ok(get_rate_limits(app, true).await)
}
