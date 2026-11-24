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
/// Antigravity is re-read at most every 5 minutes, also while the window stays
/// visible (maintainer, 2026-11-24, 《Antigravity额度读取-封号风险与新做法》).
/// The CLI decides whether Google is asked at all — only after Antigravity was
/// used, never more often than this — so calling it sooner only starts a
/// process that answers from its record. A forced refresh still calls it.
const ANTIGRAVITY_MAX_AGE: Duration = Duration::from_secs(5 * 60);

fn max_age(provider: RateLimitProvider) -> Duration {
    if provider == RateLimitProvider::Antigravity {
        ANTIGRAVITY_MAX_AGE
    } else {
        MAX_AGE
    }
}

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
            .map(|(_, at)| at.elapsed() > max_age(provider))
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
    // A login launch: no quota is read before the user's proxy is up
    // (`AppCtx::wait_launch_network`), whoever asks — the window, a
    // selection change or a key change. The window shows its loading state.
    ctx.wait_launch_network().await;
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
                codex_usage::after_live_failure(error, fallback)
            }
        };
        store_snapshot(app, snapshot, generation);
    }

    if selected.contains(&RateLimitProvider::ClaudeCode)
        && needs_refresh(app, RateLimitProvider::ClaudeCode, effective_force)
    {
        let snapshot = match claude_usage::fetch().await {
            Ok(snapshot) => snapshot,
            Err(failure) => {
                log::debug!("Claude live quota unavailable: {failure:?}");
                let cached = tauri::async_runtime::spawn_blocking(claude_usage::cached_snapshot)
                    .await
                    .ok()
                    .flatten();
                claude_usage::after_live_failure(failure, cached)
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
                    store_snapshot(app, error.snapshot(provider), generation);
                }
            }
        }
    }

    let result = cached_rate_limits(app);
    for snapshot in &result {
        test_diagnostics::record_result(snapshot);
    }
    result
}

/// What the cards show without reading anything: the last results, or an
/// empty placeholder per selected product.
pub fn cached_rate_limits(app: &AppHandle) -> Vec<ProviderRateLimit> {
    let ctx = app.state::<AppCtx>();
    let selected = selected_providers(app);
    let cache = ctx.rate_limits.lock().unwrap();
    selected
        .into_iter()
        .map(|provider| {
            if provider == RateLimitProvider::Cursor {
                return ProviderRateLimit::empty(provider, RateLimitStatus::Disabled);
            }
            cache
                .snapshots
                .get(&provider)
                .map(|(snapshot, _)| snapshot.clone())
                .unwrap_or_else(|| ProviderRateLimit::empty(provider, RateLimitStatus::NoData))
        })
        .collect::<Vec<_>>()
}

/// A settings command's answer (product selection, ZCode region or key):
/// a fresh forced read, or — while a login launch still waits for the user's
/// proxy — the cards as they are right away, so the settings window is not
/// held for up to two minutes with a stale copy of the settings. The panel
/// reads again on `settings-updated`; that read waits for the proxy and
/// picks up the new product or key (its cache entry is missing or cleared).
pub async fn after_settings_change(app: &AppHandle) -> Vec<ProviderRateLimit> {
    if !app.state::<AppCtx>().launch_network_ready() {
        return cached_rate_limits(app);
    }
    get_rate_limits(app, true).await
}

/// Products whose card starts network work: all but Cursor, a placeholder on
/// Windows (Grok's meters come from a local log, but the CLI that reads it
/// may check npm first).
pub fn reads_network(provider: RateLimitProvider) -> bool {
    provider != RateLimitProvider::Cursor
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
    Ok(after_settings_change(app).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_is_reread_at_most_every_five_minutes() {
        assert_eq!(max_age(RateLimitProvider::Antigravity), Duration::from_secs(300));
        for provider in [
            RateLimitProvider::Codex,
            RateLimitProvider::ClaudeCode,
            RateLimitProvider::KimiCode,
            RateLimitProvider::Grok,
        ] {
            assert_eq!(max_age(provider), Duration::from_secs(60));
        }
    }
}
