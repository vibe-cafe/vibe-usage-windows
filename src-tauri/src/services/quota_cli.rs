//! Typed, JSON-only bridge to the vendored CLI subscription quota contract.

use crate::services::{sync_engine, test_diagnostics, zcode_credentials};
use chrono::DateTime;
use serde::Deserialize;
use std::collections::HashSet;
use std::process::Stdio;
use std::time::Duration;
use tauri::AppHandle;
use vibe_core::quota_product::{cli_id, cli_provider, ZCodeQuotaRegion};
use vibe_core::{
    ProviderRateLimit, RateLimitEmptyReason, RateLimitLiveFailure, RateLimitMeter,
    RateLimitProvider, RateLimitSource, RateLimitStatus,
};

const SCHEMA_VERSION: u32 = 1;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    CredentialStore,
    RuntimeUnavailable,
    Timeout,
    Launch,
    ProcessFailure,
    InvalidJson,
    UnsupportedSchema,
    UnknownProduct,
    ProductMismatch,
}

impl FetchError {
    pub const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::CredentialStore => "credential_store",
            Self::RuntimeUnavailable => "cli_runtime_unavailable",
            Self::Timeout => "cli_timeout",
            Self::Launch => "cli_launch_failed",
            Self::ProcessFailure => "cli_process_failure",
            Self::InvalidJson => "invalid_json",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::UnknownProduct => "unknown_product",
            Self::ProductMismatch => "quota_product_mismatch",
        }
    }

    /// The card's fixed reason (codes shared with macOS `LiveFailure`). The
    /// process's own output is never part of it.
    pub const fn live_failure(self) -> RateLimitLiveFailure {
        match self {
            Self::CredentialStore => RateLimitLiveFailure::CredentialStoreUnavailable,
            Self::RuntimeUnavailable => RateLimitLiveFailure::CliRuntimeMissing,
            Self::Timeout => RateLimitLiveFailure::CliTimedOut,
            Self::Launch | Self::ProcessFailure => RateLimitLiveFailure::CliFailed,
            Self::InvalidJson | Self::UnknownProduct => RateLimitLiveFailure::CliUnrecognizedOutput,
            Self::UnsupportedSchema => RateLimitLiveFailure::CliSchemaMismatch,
            Self::ProductMismatch => RateLimitLiveFailure::CliNoResult,
        }
    }

    /// Every requested product's card after the whole CLI call failed.
    pub fn snapshot(self, provider: RateLimitProvider) -> ProviderRateLimit {
        ProviderRateLimit::failed(
            provider,
            RateLimitStatus::RetryableError,
            Some(RateLimitSource::Live),
            self.live_failure(),
        )
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::CredentialStore => "无法读取 Windows 凭据管理器",
            Self::RuntimeUnavailable => "本地配额运行时不可用",
            Self::Timeout => "本地配额读取超时",
            Self::Launch => "无法启动本地配额读取器",
            Self::ProcessFailure => "本地配额读取器执行失败",
            Self::InvalidJson => "本地配额返回格式无效",
            Self::UnsupportedSchema => "本地配额协议版本不兼容",
            Self::UnknownProduct => "本地配额返回了未知产品",
            Self::ProductMismatch => "本地配额返回的产品与请求不一致",
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    schema_version: u32,
    products: Vec<Product>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Product {
    id: String,
    status: String,
    #[serde(default)]
    meters: Vec<Meter>,
    plan_label: Option<String>,
    fetched_at: String,
    data_as_of: Option<String>,
    /// Optional in the CLI contract: a snapshot that predates the field (or a
    /// source that cannot say) simply omits it.
    empty_reason: Option<String>,
    /// `live` / `local` / `cache` in the CLI contract.
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meter {
    id: String,
    label: String,
    utilization: f64,
    resets_at: Option<String>,
    window_seconds: Option<f64>,
}

pub async fn fetch(
    app: &AppHandle,
    providers: &[RateLimitProvider],
    region: ZCodeQuotaRegion,
) -> Result<Vec<ProviderRateLimit>, FetchError> {
    let providers = providers
        .iter()
        .copied()
        .filter(|provider| uses_cli(*provider))
        .collect::<Vec<_>>();
    if providers.is_empty() {
        return Ok(Vec::new());
    }

    let zcode_key = if providers.contains(&RateLimitProvider::ZCode) {
        zcode_credentials::load(region).map_err(|_| FetchError::CredentialStore)
    } else {
        Ok(None)
    };
    if zcode_key.is_err() {
        test_diagnostics::record_failure(vec![RateLimitProvider::ZCode], "credential_store");
    }
    let (providers, zcode_key, mut snapshots) = isolate_credential_failure(providers, zcode_key);
    if providers.is_empty() {
        return Ok(snapshots);
    }

    let mut args = vec!["quota".to_string(), "fetch".to_string()];
    for provider in &providers {
        args.push("--product".into());
        args.push(
            cli_id(*provider)
                .expect("filtered CLI provider")
                .to_string(),
        );
    }
    args.push("--json".into());

    let mut command =
        sync_engine::quota_command(app, &args).map_err(|_| FetchError::RuntimeUnavailable)?;
    command
        .env_remove("BIGMODEL_API_KEY")
        .env_remove("Z_AI_API_KEY")
        .stdin(Stdio::null());
    if let Some(key) = zcode_key.as_deref() {
        command.env(region.environment_key(), key);
    }
    let output = tokio::time::timeout(TIMEOUT, crate::process_lifecycle::output(&mut command))
        .await
        .map_err(|_| FetchError::Timeout)?
        .map_err(|_| FetchError::Launch)?;
    if !output.status.success() {
        return Err(FetchError::ProcessFailure);
    }
    snapshots.extend(decode(&output.stdout, &providers)?);
    Ok(snapshots)
}

fn isolate_credential_failure(
    mut providers: Vec<RateLimitProvider>,
    key: Result<Option<String>, FetchError>,
) -> (
    Vec<RateLimitProvider>,
    Option<String>,
    Vec<ProviderRateLimit>,
) {
    match key {
        Ok(key) => (providers, key, Vec::new()),
        Err(_) => {
            providers.retain(|provider| *provider != RateLimitProvider::ZCode);
            (
                providers,
                None,
                vec![FetchError::CredentialStore.snapshot(RateLimitProvider::ZCode)],
            )
        }
    }
}

fn decode(
    data: &[u8],
    requested: &[RateLimitProvider],
) -> Result<Vec<ProviderRateLimit>, FetchError> {
    let envelope: Envelope = serde_json::from_slice(data).map_err(|_| FetchError::InvalidJson)?;
    if envelope.schema_version != SCHEMA_VERSION {
        return Err(FetchError::UnsupportedSchema);
    }
    let snapshots = envelope
        .products
        .into_iter()
        .map(map_product)
        .collect::<Result<Vec<_>, _>>()?;
    let expected: HashSet<_> = requested.iter().copied().collect();
    let returned: HashSet<_> = snapshots.iter().map(|snapshot| snapshot.provider).collect();
    if expected != returned || snapshots.len() != returned.len() {
        return Err(FetchError::ProductMismatch);
    }
    Ok(snapshots)
}

fn map_product(product: Product) -> Result<ProviderRateLimit, FetchError> {
    let provider = cli_provider(&product.id).ok_or(FetchError::UnknownProduct)?;
    let meters = product
        .meters
        .into_iter()
        .map(|meter| RateLimitMeter {
            id: meter.id,
            label: meter.label,
            utilization: meter.utilization.clamp(0.0, 100.0),
            resets_at: meter.resets_at.as_deref().and_then(parse_epoch),
            window_duration: meter.window_seconds.filter(|value| *value > 0.0),
        })
        .collect::<Vec<_>>();
    let status = match product.status.as_str() {
        "ok" if !meters.is_empty() => RateLimitStatus::Ok,
        "ok" | "no_data" | "unsupported" => RateLimitStatus::NoData,
        "missing_credentials" | "expired_credentials" | "unauthorized" => {
            RateLimitStatus::Unauthorized
        }
        "retryable_error" => RateLimitStatus::RetryableError,
        _ => RateLimitStatus::Error {
            message: "无法识别本地配额状态".into(),
        },
    };
    Ok(ProviderRateLimit {
        provider,
        meters,
        five_hour: None,
        seven_day: None,
        plan_label: product.plan_label,
        data_as_of: product.data_as_of.as_deref().and_then(parse_epoch),
        fetched_at: parse_epoch(&product.fetched_at),
        source_label: None,
        five_hour_not_enforced: false,
        reset_credits_count: None,
        empty_reason: product.empty_reason.as_deref().and_then(empty_reason),
        source: product.source.as_deref().and_then(source),
        // The CLI answers from its cache only after its live read reported a
        // retryable error, so a cached answer is itself the statement that
        // live failed. The specific reason exists only in `message`, which is
        // free text and never kept.
        live_failure: (product.source.as_deref() == Some("cache"))
            .then_some(RateLimitLiveFailure::CliReportedFailure),
        status,
    })
}

/// The CLI's `source` field. Unknown values stay `None` rather than guessed.
fn source(raw: &str) -> Option<RateLimitSource> {
    match raw {
        "live" => Some(RateLimitSource::Live),
        "cache" => Some(RateLimitSource::Cache),
        "local" => Some(RateLimitSource::LocalLog),
        _ => None,
    }
}

/// The CLI may name why a product answered without a window. Absent/null means
/// its snapshot predates this field — the normal path today — and an unknown
/// token means a newer CLI spoke a reason this app cannot render; both stay
/// `None` so the card reports no verdict rather than guessing.
fn empty_reason(raw: &str) -> Option<RateLimitEmptyReason> {
    match raw {
        "limitReached" => Some(RateLimitEmptyReason::LimitReached),
        "noWindow" => Some(RateLimitEmptyReason::NoWindow),
        "notEntitled" => Some(RateLimitEmptyReason::NotEntitled),
        "sessionWithoutPlanLimits" => Some(RateLimitEmptyReason::SessionWithoutPlanLimits),
        _ => None,
    }
}

fn parse_epoch(value: &str) -> Option<f64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp_millis() as f64 / 1000.0)
}

pub fn uses_cli(provider: RateLimitProvider) -> bool {
    cli_id(provider).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(ids: &[&str]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"schemaVersion": 1, "products": ids.iter().map(|id| {
            serde_json::json!({"id": id, "status": "no_data", "meters": [], "fetchedAt": "2026-09-15T00:00:00Z"})
        }).collect::<Vec<_>>()})).unwrap()
    }

    #[test]
    fn response_must_cover_exactly_the_requested_products_once() {
        let requested = [RateLimitProvider::KimiCode, RateLimitProvider::Grok];
        for ids in [
            vec![],
            vec!["grok"],
            vec!["grok", "grok"],
            vec!["kimi-code", "grok", "zcode"],
        ] {
            assert_eq!(
                decode(&envelope(&ids), &requested).unwrap_err(),
                FetchError::ProductMismatch
            );
        }
        assert_eq!(
            decode(&envelope(&["grok", "kimi-code"]), &requested)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn credential_store_failure_does_not_block_other_providers() {
        let (requested, key, failures) = isolate_credential_failure(
            vec![
                RateLimitProvider::ZCode,
                RateLimitProvider::KimiCode,
                RateLimitProvider::Grok,
            ],
            Err(FetchError::CredentialStore),
        );
        assert_eq!(
            requested,
            [RateLimitProvider::KimiCode, RateLimitProvider::Grok]
        );
        assert!(key.is_none());
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].provider, RateLimitProvider::ZCode);
        assert_eq!(failures[0].status, RateLimitStatus::RetryableError);
        assert_eq!(
            failures[0].live_failure,
            Some(RateLimitLiveFailure::CredentialStoreUnavailable)
        );
    }

    #[test]
    fn decodes_provider_neutral_meters() {
        let snapshots = decode(
            br#"{"schemaVersion":1,"products":[{"id":"grok","status":"ok","meters":[{"id":"credits","label":"7d","utilization":30,"resetsAt":"2026-09-14T00:00:00Z","windowSeconds":604800}],"planLabel":"X Premium+","fetchedAt":"2026-09-08T02:00:00Z","dataAsOf":"2026-09-08T01:00:00Z","source":"local_log"}]}"#,
            &[RateLimitProvider::Grok],
        )
        .unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].provider, RateLimitProvider::Grok);
        assert_eq!(snapshots[0].meters[0].label, "7d");
        assert_eq!(snapshots[0].meters[0].utilization, 30.0);
    }

    #[test]
    fn maps_an_optional_cli_empty_reason_and_ignores_unknown_tokens() {
        let snapshot = |extra: &str| {
            decode(
                format!(
                    r#"{{"schemaVersion":1,"products":[{{"id":"opencode-go","status":"no_data","meters":[],"fetchedAt":"2026-09-08T02:00:00Z"{extra}}}]}}"#
                )
                .as_bytes(),
                &[RateLimitProvider::OpenCodeGo],
            )
            .unwrap()
            .remove(0)
        };
        let not_entitled = snapshot(r#","emptyReason":"notEntitled""#);
        assert_eq!(not_entitled.status, RateLimitStatus::NoData);
        assert_eq!(
            not_entitled.empty_reason,
            Some(RateLimitEmptyReason::NotEntitled)
        );
        assert_eq!(
            snapshot(r#","emptyReason":"sessionWithoutPlanLimits""#).empty_reason,
            Some(RateLimitEmptyReason::SessionWithoutPlanLimits)
        );
        assert_eq!(
            snapshot(r#","emptyReason":"limitReached""#).empty_reason,
            Some(RateLimitEmptyReason::LimitReached)
        );
        // A snapshot that predates the field is the normal path, and a newer
        // CLI naming a reason this app cannot render must not error either.
        assert_eq!(snapshot("").empty_reason, None);
        assert_eq!(snapshot(r#","emptyReason":"whoKnows""#).empty_reason, None);
    }

    #[test]
    fn rejects_unknown_schema_and_product() {
        assert!(decode(br#"{"schemaVersion":2,"products":[]}"#, &[]).is_err());
        assert!(decode(
            br#"{"schemaVersion":1,"products":[{"id":"unknown","status":"ok","meters":[],"fetchedAt":"2026-09-08T02:00:00Z"}]}"#, &[]
        )
        .is_err());
    }

    #[test]
    fn credential_failures_are_typed_without_preserving_messages() {
        let snapshots = decode(
            br#"{"schemaVersion":1,"products":[{"id":"zcode","status":"unauthorized","meters":[],"fetchedAt":"2026-09-08T02:00:00Z","message":"Bearer must-not-survive"}]}"#,
            &[RateLimitProvider::ZCode],
        )
        .unwrap();
        assert_eq!(snapshots[0].status, RateLimitStatus::Unauthorized);
        assert!(!format!("{snapshots:?}").contains("must-not-survive"));
    }

    #[test]
    fn cli_source_is_mapped_and_a_cached_answer_says_live_failed() {
        let snapshot = |source: &str| {
            decode(
                format!(
                    r#"{{"schemaVersion":1,"products":[{{"id":"grok","status":"ok","meters":[{{"id":"m","label":"7d","utilization":30}}],"fetchedAt":"2026-09-08T02:00:00Z","source":"{source}","message":"ECONNRESET C:\\Users\\someone must-not-survive"}}]}}"#
                )
                .as_bytes(),
                &[RateLimitProvider::Grok],
            )
            .unwrap()
            .remove(0)
        };
        let live = snapshot("live");
        assert_eq!(live.source, Some(RateLimitSource::Live));
        assert_eq!(live.live_failure, None);
        assert_eq!(snapshot("local").source, Some(RateLimitSource::LocalLog));
        let cached = snapshot("cache");
        assert_eq!(cached.status, RateLimitStatus::Ok);
        assert_eq!(cached.source, Some(RateLimitSource::Cache));
        assert_eq!(
            cached.live_failure,
            Some(RateLimitLiveFailure::CliReportedFailure)
        );
        assert!(!format!("{cached:?}").contains("must-not-survive"));
        assert_eq!(snapshot("somethingNew").source, None);
    }

    #[test]
    fn whole_call_failures_become_fixed_retryable_reasons() {
        for (error, failure) in [
            (
                FetchError::RuntimeUnavailable,
                RateLimitLiveFailure::CliRuntimeMissing,
            ),
            (FetchError::Timeout, RateLimitLiveFailure::CliTimedOut),
            (FetchError::Launch, RateLimitLiveFailure::CliFailed),
            (FetchError::ProcessFailure, RateLimitLiveFailure::CliFailed),
            (
                FetchError::InvalidJson,
                RateLimitLiveFailure::CliUnrecognizedOutput,
            ),
            (
                FetchError::UnknownProduct,
                RateLimitLiveFailure::CliUnrecognizedOutput,
            ),
            (
                FetchError::UnsupportedSchema,
                RateLimitLiveFailure::CliSchemaMismatch,
            ),
            (
                FetchError::ProductMismatch,
                RateLimitLiveFailure::CliNoResult,
            ),
        ] {
            let snapshot = error.snapshot(RateLimitProvider::KimiCode);
            assert_eq!(snapshot.status, RateLimitStatus::RetryableError);
            assert_eq!(snapshot.source, Some(RateLimitSource::Live));
            assert_eq!(snapshot.live_failure, Some(failure));
        }
    }
}
