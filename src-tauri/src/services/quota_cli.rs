//! Typed, JSON-only bridge to the vendored CLI subscription quota contract.

use crate::services::cursor_login::{self, AfterRejection, Handover};
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
    RateLimitMeterPart, RateLimitProvider, RateLimitSignIn, RateLimitSource, RateLimitStatus,
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
    failure_reason: Option<String>,
    http_status: Option<u16>,
    /// `live` / `local` / `cache` in the CLI contract.
    #[serde(default)]
    source: Option<String>,
    /// Why an `unsupported` read was deliberately not sent (the CLI's egress
    /// check); `egressRegion` comes with `egressRegionUnsupported`.
    #[serde(default)]
    pause_reason: Option<String>,
    #[serde(default)]
    egress_region: Option<String>,
    /// Cursor only (CLI vibe-cafe/vibe-usage#210): offer the app's own
    /// sign-in, `required` or `expired`.
    #[serde(default)]
    sign_in: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meter {
    id: String,
    label: String,
    utilization: f64,
    resets_at: Option<String>,
    window_seconds: Option<f64>,
    /// Optional in the CLI contract (Cursor's Month row today).
    #[serde(default)]
    breakdown: Vec<MeterPart>,
}

#[derive(Debug, Deserialize)]
struct MeterPart {
    id: String,
    label: String,
    utilization: f64,
}

/// The CLI speaks English labels; the cards are Chinese. Only ids this app
/// knows are renamed, so a new CLI meter still renders as reported. Same map
/// as macOS `QuotaCLIBridge.displayLabel`.
fn display_label(provider: RateLimitProvider, id: &str, label: String) -> String {
    if provider != RateLimitProvider::Cursor {
        return label;
    }
    match id {
        // Same row name as Claude's pay-as-you-go spend.
        "on-demand" => "额外".into(),
        "auto" => "Cursor 模型".into(),
        "api" => "第三方模型".into(),
        _ => label,
    }
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

    // The app's own Cursor sign-in, renewed first when due, goes on stdin.
    let cursor = if providers.contains(&RateLimitProvider::Cursor) { CursorInput::Pick } else { CursorInput::None };
    let (mut answers, cursor) = run(app, &providers, zcode_key.as_deref(), region, cursor).await?;
    if let Some(handover) = cursor.filter(Handover::is_live) {
        if let Some(index) = answers.iter().position(|snapshot| snapshot.provider == RateLimitProvider::Cursor) {
            if answers[index].sign_in == Some(RateLimitSignIn::Expired) {
                answers[index] = after_cursor_rejection(app, region, handover, answers[index].clone()).await;
            } else if cursor_accepted(&answers[index]) {
                cursor_login::read_ok(app, &handover);
            }
        }
    }
    snapshots.extend(answers);
    Ok(snapshots)
}

/// Cursor rejected the handed-over sign-in (or it lapsed): renew it once and
/// read Cursor again, as the CLI does for its own stored sign-in. Cursor
/// refusing the renewal, or the renewed one, leaves the CLI's 「登录已失效」;
/// a renewal that did not complete says so, like the CLI's
/// `loginRefreshFailed`.
async fn after_cursor_rejection(
    app: &AppHandle,
    region: ZCodeQuotaRegion,
    handover: Handover,
    rejected: ProviderRateLimit,
) -> ProviderRateLimit {
    match cursor_login::after_rejection(app, &handover).await {
        AfterRejection::Renewed(next) => {
            match run(app, &[RateLimitProvider::Cursor], None, region, CursorInput::Given(&next)).await {
                Ok((mut again, _)) => {
                    let snapshot = again.remove(0);
                    if snapshot.sign_in == Some(RateLimitSignIn::Expired) {
                        // Rejected right after renewal: ended.
                        cursor_login::after_rejection(app, &next).await;
                    } else if cursor_accepted(&snapshot) {
                        cursor_login::read_ok(app, &next);
                    }
                    snapshot
                }
                Err(error) => error.snapshot(RateLimitProvider::Cursor),
            }
        }
        AfterRejection::Retry => ProviderRateLimit::failed(
            RateLimitProvider::Cursor,
            RateLimitStatus::RetryableError,
            Some(RateLimitSource::Live),
            RateLimitLiveFailure::LoginRefreshFailed,
        ),
        AfterRejection::Ended | AfterRejection::Gone => rejected,
    }
}

/// Cursor answered with this sign-in: a live read, with or without a window.
/// A cached answer, a pause or a failed request says nothing about it.
fn cursor_accepted(snapshot: &ProviderRateLimit) -> bool {
    snapshot.sign_in.is_none()
        && snapshot.live_failure.is_none()
        && match snapshot.status {
            RateLimitStatus::Ok => snapshot.source == Some(RateLimitSource::Live),
            RateLimitStatus::NoData => true,
            _ => false,
        }
}

/// Which Cursor sign-in a `quota fetch` hands over.
enum CursorInput<'a> {
    None,
    /// The kept one, renewed first when due (`cursor_login::for_cli`).
    Pick,
    Given(&'a Handover),
}

/// One `quota fetch` for `providers`, and the Cursor sign-in it was handed.
/// The ZCode key goes in its region's variable, the Cursor sign-in on stdin;
/// neither in an argument.
async fn run(
    app: &AppHandle,
    providers: &[RateLimitProvider],
    zcode_key: Option<&str>,
    region: ZCodeQuotaRegion,
    cursor: CursorInput<'_>,
) -> Result<(Vec<ProviderRateLimit>, Option<Handover>), FetchError> {
    // Renewed first when due, before the command reads the proxy: a renewal
    // may take a minute.
    let cursor = match cursor {
        CursorInput::None => None,
        CursorInput::Pick => cursor_login::for_cli(app).await,
        CursorInput::Given(handover) => Some(handover.clone()),
    };
    let mut args = vec!["quota".to_string(), "fetch".to_string()];
    for provider in providers {
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
    if let Some(key) = zcode_key {
        command.env(region.environment_key(), key);
    }
    // Checked again right before the start: reading the proxy, PAC included,
    // can take seconds, and a sign-out meanwhile must hold. Only a product
    // list with Cursor carries it.
    let cursor = if providers.contains(&RateLimitProvider::Cursor) { cursor_login::recheck(cursor) } else { None };
    if cursor.is_some() {
        command.arg("--cursor-login-stdin");
    }
    let output = match &cursor {
        Some(handover) => {
            tokio::time::timeout(TIMEOUT, crate::process_lifecycle::output_with_input(&mut command, &handover.input)).await
        }
        None => tokio::time::timeout(TIMEOUT, crate::process_lifecycle::output(&mut command)).await,
    }
    .map_err(|_| FetchError::Timeout)?
    .map_err(|_| FetchError::Launch)?;
    if !output.status.success() {
        return Err(FetchError::ProcessFailure);
    }
    Ok((decode(&output.stdout, providers)?, cursor))
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
    use RateLimitLiveFailure as Failure;

    let provider = cli_provider(&product.id).ok_or(FetchError::UnknownProduct)?;
    let meters = product
        .meters
        .into_iter()
        .map(|meter| RateLimitMeter {
            label: display_label(provider, &meter.id, meter.label),
            id: meter.id,
            utilization: meter.utilization.clamp(0.0, 100.0),
            resets_at: meter.resets_at.as_deref().and_then(parse_epoch),
            window_duration: meter.window_seconds.filter(|value| *value > 0.0),
            breakdown: meter
                .breakdown
                .into_iter()
                .map(|part| RateLimitMeterPart {
                    label: display_label(provider, &part.id, part.label),
                    utilization: part.utilization.clamp(0.0, 100.0),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let mut failure = product
        .failure_reason
        .as_deref()
        .and_then(|reason| live_failure(reason, product.http_status));
    let status = match product.status.as_str() {
        // Only OpenCode can renew this sign-in. Keep its settled card and actions.
        "missing_credentials" | "expired_credentials"
            if provider == RateLimitProvider::OpenCodeGo =>
        {
            failure.get_or_insert(if product.status == "expired_credentials" {
                Failure::CredentialsExpired
            } else {
                Failure::CredentialsNotFound
            });
            RateLimitStatus::NoData
        }
        "ok" if !meters.is_empty() => RateLimitStatus::Ok,
        "ok" | "no_data" | "unsupported" => RateLimitStatus::NoData,
        "missing_credentials" => {
            failure.get_or_insert(if provider == RateLimitProvider::Antigravity {
                Failure::ProgramNotFound
            } else {
                Failure::CredentialsNotFound
            });
            RateLimitStatus::Unauthorized
        }
        "expired_credentials" | "unauthorized" => {
            failure.get_or_insert(Failure::CredentialsRejected);
            RateLimitStatus::Unauthorized
        }
        "retryable_error" => RateLimitStatus::RetryableError,
        _ => RateLimitStatus::Error {
            message: "无法识别本地配额状态".into(),
        },
    };
    if matches!(product.status.as_str(), "ok" | "unsupported") {
        failure = pause_failure(
            product.pause_reason.as_deref(),
            product.egress_region.as_deref(),
        )
        .or(failure);
    }
    if product.source.as_deref() == Some("cache") {
        failure.get_or_insert(Failure::CliReportedFailure);
    }
    let sign_in = match product.sign_in.as_deref() {
        Some("required") if provider == RateLimitProvider::Cursor => Some(RateLimitSignIn::Required),
        Some("expired") if provider == RateLimitProvider::Cursor => Some(RateLimitSignIn::Expired),
        _ => None,
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
        live_failure: failure,
        sign_in,
        status,
    })
}

fn live_failure(reason: &str, http_status: Option<u16>) -> Option<RateLimitLiveFailure> {
    use RateLimitLiveFailure::*;
    Some(match reason {
        "credentialsMissing" => CredentialsNotFound,
        "credentialsRejected" => CredentialsRejected,
        "credentialsUnreadable" => CredentialsUnreadable,
        "credentialsEncrypted" => CredentialsEncrypted,
        "loginRefreshFailed" => LoginRefreshFailed,
        "programNotFound" => ProgramNotFound,
        "programOutdated" => ProgramOutdated,
        "programNoAnswer" => ProgramNoAnswer,
        "timedOut" => EndpointTimedOut,
        "unreachable" => EndpointUnreachable,
        "httpStatus" => EndpointHttpStatus {
            status: http_status.filter(|s| (100..=599).contains(s)),
        },
        "unrecognizedResponse" => EndpointUnrecognized,
        "localRecordUnreadable" => LocalRecordUnreadable,
        "internalError" => CliFailed,
        _ => return None,
    })
}

/// The CLI's `pauseReason`, as the card's own fixed reason. Unknown codes stay
/// `None` (the card then says nothing was read, as before).
fn pause_failure(reason: Option<&str>, region: Option<&str>) -> Option<RateLimitLiveFailure> {
    match reason? {
        "egressRegionUnsupported" => Some(RateLimitLiveFailure::EgressRegionUnsupported {
            region: vibe_core::EgressRegion::from_code(region.unwrap_or_default()),
        }),
        "egressUnconfirmed" => Some(RateLimitLiveFailure::EgressUnconfirmed),
        "proxyNotDetected" => Some(RateLimitLiveFailure::ProxyNotDetected),
        "proxyUnsupported" => Some(RateLimitLiveFailure::ProxyUnsupported),
        _ => None,
    }
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
        "periodEnded" => Some(RateLimitEmptyReason::PeriodEnded),
        "noLocalRecord" => Some(RateLimitEmptyReason::NoLocalRecord),
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
    fn cursor_month_carries_its_split_and_on_demand_reads_like_claude_extra() {
        let snapshots = decode(
            br#"{"schemaVersion":1,"products":[{"id":"cursor","status":"ok","meters":[{"id":"plan","label":"Month","utilization":48,"resetsAt":"2026-12-01T00:00:00.000Z","windowSeconds":2678400,"breakdown":[{"id":"auto","label":"Cursor models","utilization":4},{"id":"api","label":"Third-party models","utilization":88}]},{"id":"on-demand","label":"On-demand","utilization":25}],"planLabel":"Pro","fetchedAt":"2026-10-23T00:00:00Z","source":"live"}]}"#,
            &[RateLimitProvider::Cursor],
        )
        .unwrap();
        let cursor = &snapshots[0];
        assert_eq!(cursor.provider, RateLimitProvider::Cursor);
        assert_eq!(cursor.status, RateLimitStatus::Ok);
        assert_eq!(cursor.plan_label.as_deref(), Some("Pro"));
        assert_eq!(
            cursor.meters.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(),
            ["Month", "额外"]
        );
        assert_eq!(cursor.meters[0].window_duration, Some(2678400.0));
        assert!(cursor.meters[0].resets_at.is_some());
        assert_eq!(
            cursor.meters[0].breakdown,
            vec![
                RateLimitMeterPart {
                    label: "Cursor 模型".into(),
                    utilization: 4.0
                },
                RateLimitMeterPart {
                    label: "第三方模型".into(),
                    utilization: 88.0
                },
            ]
        );
        assert!(cursor.meters[1].breakdown.is_empty());
        assert_eq!(cursor.live_failure, None);
    }

    #[test]
    fn only_cursor_meter_ids_are_renamed() {
        let snapshots = decode(
            br#"{"schemaVersion":1,"products":[{"id":"kimi-code","status":"ok","meters":[{"id":"on-demand","label":"On-demand","utilization":10,"breakdown":[{"id":"auto","label":"Auto","utilization":5}]}],"fetchedAt":"2026-10-23T00:00:00Z"}]}"#,
            &[RateLimitProvider::KimiCode],
        )
        .unwrap();
        assert_eq!(snapshots[0].meters[0].label, "On-demand");
        assert_eq!(snapshots[0].meters[0].breakdown[0].label, "Auto");
    }

    #[test]
    fn cursor_login_states_map_like_the_macos_card() {
        let product = |status: &str| {
            let payload = format!(
                r#"{{"schemaVersion":1,"products":[{{"id":"cursor","status":"{status}","meters":[],"fetchedAt":"2026-11-25T00:00:00.000Z","source":"live","message":"free text the card never shows"}}]}}"#
            );
            decode(payload.as_bytes(), &[RateLimitProvider::Cursor])
                .unwrap()
                .remove(0)
        };
        // Nothing was sent (e.g. a proxy the CLI cannot follow): no data, no
        // retry, no sign-in prompt, and the message text is not kept.
        let unsupported = product("unsupported");
        assert_eq!(unsupported.status, RateLimitStatus::NoData);
        assert_eq!(unsupported.live_failure, None);
        assert!(unsupported.meters.is_empty());
        // An encrypted Cursor login: the card says so (#190 reason), still no
        // sign-in prompt, since signing in again writes the same ciphertext.
        let encrypted = decode(
            br#"{"schemaVersion":1,"products":[{"id":"cursor","status":"unsupported","failureReason":"credentialsEncrypted","meters":[],"fetchedAt":"2026-11-25T00:00:00.000Z","source":"live"}]}"#,
            &[RateLimitProvider::Cursor],
        )
        .unwrap()
        .remove(0);
        assert_eq!(encrypted.status, RateLimitStatus::NoData);
        assert_eq!(encrypted.live_failure, Some(RateLimitLiveFailure::CredentialsEncrypted));
        // No login found, or cursor.com rejected it: sign in to Cursor.
        for status in ["missing_credentials", "unauthorized"] {
            assert_eq!(product(status).status, RateLimitStatus::Unauthorized);
        }
        assert_eq!(product("retryable_error").status, RateLimitStatus::RetryableError);
    }

    #[test]
    fn a_cursor_sign_in_request_is_kept_only_as_the_cli_said_it() {
        let product = |id: &str, extra: &str| {
            let payload = format!(
                r#"{{"schemaVersion":1,"products":[{{"id":"{id}","status":"unsupported","meters":[],"fetchedAt":"2026-11-26T00:00:00.000Z","source":"live"{extra}}}]}}"#
            );
            let provider = cli_provider(id).unwrap();
            decode(payload.as_bytes(), &[provider]).unwrap().remove(0)
        };
        // CLI 0.15.10+: Cursor's local login is encrypted, offer the sign-in.
        let required = product("cursor", r#","failureReason":"credentialsEncrypted","signIn":"required""#);
        assert_eq!(required.status, RateLimitStatus::NoData);
        assert_eq!(required.sign_in, Some(RateLimitSignIn::Required));
        assert_eq!(required.live_failure, Some(RateLimitLiveFailure::CredentialsEncrypted));
        // The app's own sign-in was ended by Cursor.
        let expired = product("cursor", r#","failureReason":"credentialsRejected","signIn":"expired""#);
        assert_eq!(expired.sign_in, Some(RateLimitSignIn::Expired));
        assert_eq!(expired.live_failure, Some(RateLimitLiveFailure::CredentialsRejected));
        // An older CLI says nothing; an unknown value or another product is ignored.
        assert_eq!(product("cursor", "").sign_in, None);
        assert_eq!(product("cursor", r#","signIn":"later""#).sign_in, None);
        assert_eq!(product("grok", r#","signIn":"required""#).sign_in, None);
        // Serialized for the cards only when present.
        let json = serde_json::to_value(&expired).unwrap();
        assert_eq!(json["signIn"], "expired");
        assert!(serde_json::to_value(product("cursor", "")).unwrap().get("signIn").is_none());
    }

    #[test]
    fn only_a_live_answer_shows_cursor_took_the_sign_in() {
        let cursor = |extra: &str| {
            let payload = format!(
                r#"{{"schemaVersion":1,"products":[{{"id":"cursor","fetchedAt":"2026-11-26T00:00:00.000Z"{extra}}}]}}"#
            );
            decode(payload.as_bytes(), &[RateLimitProvider::Cursor]).unwrap().remove(0)
        };
        let month = r#","meters":[{"id":"plan","label":"Month","utilization":48}]"#;
        assert!(cursor_accepted(&cursor(&format!(r#","status":"ok","source":"live"{month}"#))));
        // Answered, without a window.
        assert!(cursor_accepted(&cursor(r#","status":"no_data","source":"live""#)));
        // A cached answer after a failed request, a failed request, a pause or a
        // sign-in request say nothing about whether Cursor took it.
        assert!(!cursor_accepted(&cursor(&format!(r#","status":"ok","source":"cache"{month}"#))));
        assert!(!cursor_accepted(&cursor(r#","status":"retryable_error","source":"live","failureReason":"unreachable""#)));
        assert!(!cursor_accepted(&cursor(r#","status":"unsupported","source":"live","pauseReason":"proxyUnsupported""#)));
        assert!(!cursor_accepted(&cursor(r#","status":"unsupported","failureReason":"credentialsRejected","signIn":"expired""#)));
    }

    #[test]
    fn antigravity_answers_reach_its_card_and_a_missing_agy_asks_to_install() {
        let ok = decode(
            br#"{"schemaVersion":1,"products":[{"id":"antigravity","status":"ok","meters":[{"id":"gemini-5h","label":"5h","utilization":100,"windowSeconds":18000},{"id":"gemini-weekly","label":"7d","utilization":25,"resetsAt":"2026-11-23T00:00:00.000Z","windowSeconds":604800}],"fetchedAt":"2026-11-19T02:00:00.000Z","source":"live"}]}"#,
            &[RateLimitProvider::Antigravity],
        )
        .unwrap()
        .remove(0);
        assert_eq!(ok.provider, RateLimitProvider::Antigravity);
        assert_eq!(ok.status, RateLimitStatus::Ok);
        assert_eq!(
            ok.meters.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(),
            ["5h", "7d"]
        );
        // An older CLI only reported the missing_credentials status.
        let missing = decode(
            br#"{"schemaVersion":1,"products":[{"id":"antigravity","status":"missing_credentials","meters":[],"fetchedAt":"2026-11-19T02:00:00.000Z","message":"Install agy and sign in, then retry"}]}"#,
            &[RateLimitProvider::Antigravity],
        )
        .unwrap()
        .remove(0);
        assert_eq!(missing.status, RateLimitStatus::Unauthorized);
        assert_eq!(missing.live_failure, Some(RateLimitLiveFailure::ProgramNotFound));
    }

    #[test]
    fn a_cli_pause_reason_becomes_the_cards_reason() {
        let paused = |reason: &str, region: &str| {
            let json = format!(
                r#"{{"schemaVersion":1,"products":[{{"id":"antigravity","status":"unsupported","meters":[],"fetchedAt":"2026-11-24T12:00:00.000Z","source":"live","message":"Paused","pauseReason":"{reason}"{region}}}]}}"#
            );
            decode(json.as_bytes(), &[RateLimitProvider::Antigravity]).unwrap().remove(0)
        };
        let region = paused("egressRegionUnsupported", r#","egressRegion":"HK""#);
        assert_eq!(region.status, RateLimitStatus::NoData);
        assert_eq!(
            region.live_failure,
            Some(RateLimitLiveFailure::EgressRegionUnsupported { region: vibe_core::EgressRegion::HK })
        );
        assert_eq!(paused("egressUnconfirmed", "").live_failure, Some(RateLimitLiveFailure::EgressUnconfirmed));
        assert_eq!(paused("proxyNotDetected", "").live_failure, Some(RateLimitLiveFailure::ProxyNotDetected));
        let proxy = paused("proxyUnsupported", "");
        assert_eq!(proxy.status, RateLimitStatus::NoData);
        assert_eq!(proxy.live_failure, Some(RateLimitLiveFailure::ProxyUnsupported));
        assert!(proxy.live_failure.unwrap().is_pause());
        assert_eq!(paused("somethingNew", "").live_failure, None);
    }

    #[test]
    fn opencode_go_login_states_are_settled_reasons_not_a_relogin_request() {
        let snapshot = |status: &str| {
            decode(
                format!(
                    r#"{{"schemaVersion":1,"products":[{{"id":"opencode-go","status":"{status}","meters":[],"fetchedAt":"2026-11-24T02:00:00Z"}}]}}"#
                )
                .as_bytes(),
                &[RateLimitProvider::OpenCodeGo],
            )
            .unwrap()
            .remove(0)
        };
        let missing = snapshot("missing_credentials");
        assert_eq!(missing.status, RateLimitStatus::NoData);
        assert_eq!(
            missing.live_failure,
            Some(RateLimitLiveFailure::CredentialsNotFound)
        );
        let expired = snapshot("expired_credentials");
        assert_eq!(expired.status, RateLimitStatus::NoData);
        assert_eq!(
            expired.live_failure,
            Some(RateLimitLiveFailure::CredentialsExpired)
        );
        // OpenCode refused the login: only OpenCode can issue a new one.
        let rejected = snapshot("unauthorized");
        assert_eq!(rejected.status, RateLimitStatus::Unauthorized);
        assert_eq!(rejected.live_failure, Some(RateLimitLiveFailure::CredentialsRejected));
    }

    #[test]
    fn cli_login_states_distinguish_absence_from_rejection() {
        use RateLimitLiveFailure::*;
        for id in ["kimi-code", "zcode", "antigravity", "grok"] {
            for (status, failure) in [
                ("missing_credentials", if id == "antigravity" { ProgramNotFound } else { CredentialsNotFound }),
                ("expired_credentials", CredentialsRejected),
                ("unauthorized", CredentialsRejected),
            ] {
                let mut product = serde_json::json!({ "id": id, "status": status, "fetchedAt": "2026-11-25T00:00:00Z" });
                let snapshot = map_product(serde_json::from_value(product.clone()).unwrap()).unwrap();
                assert_eq!(snapshot.status, RateLimitStatus::Unauthorized);
                assert_eq!(snapshot.live_failure, Some(failure));
                // A known code takes precedence over the legacy status fallback.
                product["failureReason"] = "credentialsUnreadable".into();
                let snapshot = map_product(serde_json::from_value(product).unwrap()).unwrap();
                assert_eq!(snapshot.live_failure, Some(CredentialsUnreadable));
            }
        }
    }

    #[test]
    fn maps_only_fixed_cli_failure_reasons() {
        use RateLimitLiveFailure::*;
        for (code, expected) in [
            ("credentialsMissing", CredentialsNotFound),
            ("credentialsRejected", CredentialsRejected),
            ("credentialsUnreadable", CredentialsUnreadable),
            ("credentialsEncrypted", CredentialsEncrypted),
            ("loginRefreshFailed", LoginRefreshFailed),
            ("programNotFound", ProgramNotFound),
            ("programOutdated", ProgramOutdated),
            ("programNoAnswer", ProgramNoAnswer),
            ("timedOut", EndpointTimedOut),
            ("unreachable", EndpointUnreachable),
            ("httpStatus", EndpointHttpStatus { status: Some(502) }),
            ("unrecognizedResponse", EndpointUnrecognized),
            ("localRecordUnreadable", LocalRecordUnreadable),
            ("internalError", CliFailed),
        ] {
            assert_eq!(live_failure(code, Some(502)), Some(expected));
        }
        for status in [None, Some(99), Some(600)] {
            assert_eq!(live_failure("httpStatus", status), Some(EndpointHttpStatus { status: None }));
        }
        assert_eq!(live_failure("unknown", Some(502)), None);
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
        assert_eq!(snapshot(r#","emptyReason":"noWindow""#).empty_reason, Some(RateLimitEmptyReason::NoWindow));
        assert_eq!(snapshot(r#","emptyReason":"periodEnded""#).empty_reason, Some(RateLimitEmptyReason::PeriodEnded));
        assert_eq!(snapshot(r#","emptyReason":"noLocalRecord""#).empty_reason, Some(RateLimitEmptyReason::NoLocalRecord));
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
        let snapshot = |source: &str, extra: &str| {
            decode(
                format!(
                    r#"{{"schemaVersion":1,"products":[{{"id":"grok","status":"ok","meters":[{{"id":"m","label":"7d","utilization":30}}],"fetchedAt":"2026-09-08T02:00:00Z","source":"{source}","message":"ECONNRESET C:\\Users\\someone must-not-survive"{extra}}}]}}"#
                )
                .as_bytes(),
                &[RateLimitProvider::Grok],
            )
            .unwrap()
            .remove(0)
        };
        let live = snapshot("live", "");
        assert_eq!(live.source, Some(RateLimitSource::Live));
        assert_eq!(live.live_failure, None);
        assert_eq!(snapshot("local", "").source, Some(RateLimitSource::LocalLog));
        let cached = snapshot("cache", "");
        assert_eq!(cached.status, RateLimitStatus::Ok);
        assert_eq!(cached.source, Some(RateLimitSource::Cache));
        assert_eq!(
            cached.live_failure,
            Some(RateLimitLiveFailure::CliReportedFailure)
        );
        assert!(!format!("{cached:?}").contains("must-not-survive"));
        assert_eq!(snapshot("somethingNew", "").source, None);
        for source in ["cache", "local"] {
            let fallback = snapshot(source, r#","failureReason":"httpStatus","httpStatus":502"#);
            assert_eq!(fallback.status, RateLimitStatus::Ok);
            assert_eq!(fallback.live_failure, Some(RateLimitLiveFailure::EndpointHttpStatus { status: Some(502) }));
        }
        let paused = snapshot("local", r#","pauseReason":"proxyUnsupported""#);
        assert_eq!(paused.status, RateLimitStatus::Ok);
        assert_eq!(paused.meters[0].utilization, 30.0);
        assert_eq!(paused.live_failure, Some(RateLimitLiveFailure::ProxyUnsupported));
        assert_eq!(snapshot("cache", r#","failureReason":"future""#).live_failure, cached.live_failure);
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
