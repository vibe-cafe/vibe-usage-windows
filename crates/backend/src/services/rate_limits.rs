//! Subscription-quota coordination. Native Codex/Claude readers stay in
//! place; Kimi/ZCode/Grok use the versioned vendored-CLI contract.

use crate::services::{claude_usage, codex_usage, quota_cli, test_diagnostics};
use crate::Backend;
use serde::Serialize;
use std::time::Duration;
use vibe_core::rate_limit::codex;
use vibe_core::statusline_hook::StatuslineHook;
use vibe_core::{ProviderRateLimit, RateLimitLiveFailure, RateLimitProvider, RateLimitStatus};

const MAX_AGE: Duration = Duration::from_secs(60);
/// Antigravity and Grok are re-read at most every 5 minutes, also while the
/// window stays visible. Antigravity (maintainer, 2026-11-24,
/// 《Antigravity额度读取-封号风险与新做法》): the CLI decides whether Google is
/// asked at all — only after Antigravity was used, never more often than this —
/// so calling it sooner only starts a process that answers from its record.
/// Grok (maintainer, 2026-11-25, 《Grok主动读取-审批门方案》): every read starts
/// the official `grok` CLI, which asks xAI with the user's own sign-in, so a
/// window left open must not start it every minute. A forced refresh still
/// reads both.
const SLOW_MAX_AGE: Duration = Duration::from_secs(5 * 60);

fn max_age(provider: RateLimitProvider) -> Duration {
    match provider {
        RateLimitProvider::Antigravity | RateLimitProvider::Grok => SLOW_MAX_AGE,
        _ => MAX_AGE,
    }
}

pub fn statusline_hook(app: &Backend) -> StatuslineHook {
    StatuslineHook::new(crate::services::sync_engine::node_for_statusline(app))
}

fn selected_providers(app: &Backend) -> Vec<RateLimitProvider> {
    let ctx = app.ctx();
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

fn needs_refresh(app: &Backend, provider: RateLimitProvider, force: bool) -> bool {
    let ctx = app.ctx();
    let cache = ctx.rate_limits.lock().unwrap();
    // A read the egress check paused is tried again at the next refresh (the
    // check keeps its own one-minute answer, so nothing extra goes out while
    // the route stays direct).
    force
        || cache
            .snapshots
            .get(&provider)
            .map(|(snapshot, at)| at.elapsed() > max_age(provider) || recheck_egress(snapshot))
            .unwrap_or(true)
}

// A read the egress check paused is read again at the next refresh, whatever
// its status (numbers kept from a local record included). `ProxyUnsupported`
// is a pause too, but it keeps the normal interval: as before #190.
fn recheck_egress(snapshot: &ProviderRateLimit) -> bool {
    matches!(
        snapshot.live_failure,
        Some(
            RateLimitLiveFailure::EgressRegionUnsupported { .. }
                | RateLimitLiveFailure::EgressUnconfirmed
                | RateLimitLiveFailure::ProxyNotDetected
        )
    )
}

pub async fn get_rate_limits(app: &Backend, force: bool) -> Vec<ProviderRateLimit> {
    let ctx = app.ctx();
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
    ctx.rate_limits.lock().unwrap().refreshing = true;
    let _ = app.emit("rate-limits-updated", ());
    // Lock order is settings -> cache, also used by credential mutation commands.
    let (region, generation) = {
        let settings = ctx.settings.lock().unwrap();
        let cache = ctx.rate_limits.lock().unwrap();
        (settings.z_code_quota_region, cache.generation())
    };
    let selected = selected_providers(app);
    test_diagnostics::record_refresh_started(&app.options().local_data, selected.clone());
    let due: Vec<_> = selected
        .into_iter()
        .filter(|p| needs_refresh(app, *p, effective_force))
        .collect();
    if !due.is_empty() {
        let snapshots = app.0.network.quotas(app, &due, region).await;
        let mut cache = ctx.rate_limits.lock().unwrap();
        for snapshot in snapshots {
            cache.store(snapshot, generation);
        }
    }

    let result = cached_rate_limits(app).snapshots;
    ctx.rate_limits.lock().unwrap().refreshing = false;
    let _ = app.emit("rate-limits-updated", &result);
    for snapshot in &result {
        test_diagnostics::record_result(&app.options().local_data, snapshot);
    }
    result
}

pub(crate) async fn read_due(
    app: &Backend,
    due: &[RateLimitProvider],
    region: vibe_core::quota_product::ZCodeQuotaRegion,
) -> Vec<ProviderRateLimit> {
    // Read egress settings immediately before each provider's request.
    let egress = || app.ctx().settings.lock().unwrap().egress();
    let mut result = Vec::new();
    if due.contains(&RateLimitProvider::Codex) {
        let http = app.ctx().http.clone();
        let live = codex_usage::fetch(&http, egress()).await;
        let snapshot = match live {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let fallback = tokio::task::spawn_blocking(codex::read)
                    .await
                    .unwrap_or_else(|_| {
                        ProviderRateLimit::empty(RateLimitProvider::Codex, RateLimitStatus::NoData)
                    });
                codex_usage::after_live_failure(error, fallback)
            }
        };
        result.push(snapshot);
    }

    if due.contains(&RateLimitProvider::ClaudeCode) {
        let snapshot = match claude_usage::fetch(egress()).await {
            Ok(snapshot) => snapshot,
            Err(failure) => {
                log::debug!("Claude live quota unavailable: {failure:?}");
                let cached = tokio::task::spawn_blocking(claude_usage::cached_snapshot)
                    .await
                    .ok()
                    .flatten();
                claude_usage::after_live_failure(failure, cached)
            }
        };
        result.push(snapshot);
    }

    let cli_providers = due
        .iter()
        .copied()
        .filter(|provider| quota_cli::uses_cli(*provider))
        .collect::<Vec<_>>();
    if !cli_providers.is_empty() {
        match quota_cli::fetch(app, &cli_providers, region).await {
            Ok(snapshots) => {
                result.extend(snapshots);
            }
            Err(error) => {
                log::debug!("CLI quota unavailable: {error}");
                test_diagnostics::record_failure(
                    &app.options().local_data,
                    cli_providers.clone(),
                    error.diagnostic_code(),
                );
                for provider in cli_providers {
                    result.push(error.snapshot(provider));
                }
            }
        }
    }

    result
}

#[derive(Serialize)]
pub struct CachedRateLimits {
    snapshots: Vec<ProviderRateLimit>,
    refreshing: bool,
}

/// What the cards show without reading anything: the last results, or an
/// empty placeholder per selected product.
pub fn cached_rate_limits(app: &Backend) -> CachedRateLimits {
    let ctx = app.ctx();
    let selected = selected_providers(app);
    let cache = ctx.rate_limits.lock().unwrap();
    let snapshots = selected
        .into_iter()
        .map(|provider| {
            cache
                .snapshots
                .get(&provider)
                .map(|(snapshot, _)| snapshot.clone())
                .unwrap_or_else(|| ProviderRateLimit::empty(provider, RateLimitStatus::NoData))
        })
        .collect();
    CachedRateLimits {
        snapshots,
        refreshing: cache.refreshing || !ctx.launch_network_ready(),
    }
}

/// A settings command's answer (product selection, ZCode region or key):
/// a fresh forced read, or — while a login launch still waits for the user's
/// proxy — the cards as they are right away, so the settings window is not
/// held for up to two minutes. Queue a read behind the gate so the new
/// selection or credential is picked up even if the panel is hidden.
pub async fn after_settings_change(app: &Backend) -> Vec<ProviderRateLimit> {
    if !app.ctx().launch_network_ready() {
        app.refresh_quotas();
        return cached_rate_limits(app).snapshots;
    }
    app.quotas(true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egress_pauses_are_reread_at_the_next_refresh_whatever_their_status() {
        use RateLimitLiveFailure::{EgressUnconfirmed, ProxyUnsupported};
        let snapshot =
            |provider, status, reason| ProviderRateLimit::failed(provider, status, None, reason);
        assert!(recheck_egress(&snapshot(
            RateLimitProvider::ClaudeCode,
            RateLimitStatus::Ok,
            EgressUnconfirmed
        )));
        assert!(recheck_egress(&snapshot(
            RateLimitProvider::Antigravity,
            RateLimitStatus::NoData,
            EgressUnconfirmed
        )));
        assert!(recheck_egress(&snapshot(
            RateLimitProvider::Grok,
            RateLimitStatus::Ok,
            EgressUnconfirmed
        )));
        assert!(!recheck_egress(&snapshot(
            RateLimitProvider::Antigravity,
            RateLimitStatus::NoData,
            ProxyUnsupported
        )));
    }

    #[test]
    fn antigravity_and_grok_are_reread_at_most_every_five_minutes() {
        assert_eq!(
            max_age(RateLimitProvider::Antigravity),
            Duration::from_secs(300)
        );
        assert_eq!(max_age(RateLimitProvider::Grok), Duration::from_secs(300));
        for provider in [
            RateLimitProvider::Codex,
            RateLimitProvider::ClaudeCode,
            RateLimitProvider::KimiCode,
            RateLimitProvider::ZCode,
        ] {
            assert_eq!(max_age(provider), Duration::from_secs(60));
        }
    }
}
