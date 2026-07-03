//! Rate-limit coordination — port of Services/RateLimitCoordinator.swift.
//! 60s debounce per provider; force refresh bypasses it.

use crate::state::AppCtx;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use vibe_core::rate_limit::{claude, codex};
use vibe_core::statusline_hook::StatuslineHook;
use vibe_core::{ProviderRateLimit, RateLimitProvider, RateLimitStatus};

const MAX_AGE: Duration = Duration::from_secs(60);

pub fn statusline_hook(app: &AppHandle) -> StatuslineHook {
    StatuslineHook::new(crate::services::sync_engine::node_for_statusline(app))
}

pub async fn get_rate_limits(app: &AppHandle, force: bool) -> Vec<ProviderRateLimit> {
    let claude_enabled = {
        let ctx = app.state::<AppCtx>();
        ctx.settings.lock().unwrap().claude_rate_limit_enabled
    };

    let needs_codex = {
        let ctx = app.state::<AppCtx>();
        let cache = ctx.rate_limits.lock().unwrap();
        force
            || cache
                .codex
                .as_ref()
                .map(|(_, at)| at.elapsed() > MAX_AGE)
                .unwrap_or(true)
    };
    if needs_codex {
        let snapshot = tauri::async_runtime::spawn_blocking(codex::read)
            .await
            .unwrap_or_else(|_| {
                ProviderRateLimit::empty(RateLimitProvider::Codex, RateLimitStatus::NoData)
            });
        let ctx = app.state::<AppCtx>();
        ctx.rate_limits.lock().unwrap().codex = Some((snapshot, Instant::now()));
    }

    let needs_claude = {
        let ctx = app.state::<AppCtx>();
        let cache = ctx.rate_limits.lock().unwrap();
        force
            || cache
                .claude
                .as_ref()
                .map(|(_, at)| at.elapsed() > MAX_AGE)
                .unwrap_or(true)
    };
    if needs_claude {
        let capture_file = statusline_hook(app).rate_limit_file();
        let snapshot = tauri::async_runtime::spawn_blocking(move || {
            claude::read(&capture_file, claude_enabled)
        })
        .await
        .unwrap_or_else(|_| {
            ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Disabled)
        });
        let ctx = app.state::<AppCtx>();
        ctx.rate_limits.lock().unwrap().claude = Some((snapshot, Instant::now()));
    }

    let ctx = app.state::<AppCtx>();
    let cache = ctx.rate_limits.lock().unwrap();
    vec![
        cache
            .codex
            .as_ref()
            .map(|(s, _)| s.clone())
            .unwrap_or_else(|| {
                ProviderRateLimit::empty(RateLimitProvider::Codex, RateLimitStatus::NoData)
            }),
        cache
            .claude
            .as_ref()
            .map(|(s, _)| s.clone())
            .unwrap_or_else(|| {
                ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Disabled)
            }),
    ]
}

/// Enable Claude quota capture: install the statusline wrapper, persist the
/// opt-in, then poll briefly so a single 启用 click populates the card
/// (mirrors AppState.enableClaudeRateLimit).
pub async fn enable_claude(app: &AppHandle) -> Result<Vec<ProviderRateLimit>, String> {
    let hook = statusline_hook(app);
    hook.install().map_err(|e| e.to_string())?;

    {
        let ctx = app.state::<AppCtx>();
        ctx.settings.lock().unwrap().claude_rate_limit_enabled = true;
        ctx.save_settings();
    }

    let mut limits = get_rate_limits(app, true).await;
    for _ in 0..6 {
        let ok = limits
            .iter()
            .any(|l| l.provider == RateLimitProvider::ClaudeCode && l.status == RateLimitStatus::Ok);
        if ok {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        limits = get_rate_limits(app, true).await;
    }
    Ok(limits)
}
