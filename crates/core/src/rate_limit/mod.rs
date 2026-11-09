//! Subscription quota (订阅配额) types + readers — port of Models/RateLimit.swift,
//! Services/CodexRateLimitReader.swift and Services/ClaudeRateLimitReader.swift.

pub mod claude;
pub mod claude_desktop;
pub mod codex;

use serde::{Deserialize, Serialize};

/// One subscription window (5h or 7d). Serialized camelCase for the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    /// 0-100
    pub utilization: f64,
    /// epoch seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<f64>,
    /// seconds; present → the elapsed-time bar can render
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_duration: Option<f64>,
}

/// Provider-neutral quota meter. CLI-backed products are not required to use
/// Codex's exact 5h/7d shape, so the UI renders these labels directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitMeter {
    pub id: String,
    pub label: String,
    pub utilization: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_duration: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RateLimitProvider {
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "claudeCode")]
    ClaudeCode,
    #[serde(rename = "kimi-code")]
    KimiCode,
    #[serde(rename = "zcode")]
    ZCode,
    #[serde(rename = "grok")]
    Grok,
    /// CLI-backed like Kimi/ZCode/Grok: OpenCode's credential store is a
    /// sqlite table, so the app never reads it itself.
    #[serde(rename = "opencode-go")]
    OpenCodeGo,
    #[serde(rename = "cursor")]
    Cursor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RateLimitStatus {
    Ok,
    NoData,
    Disabled,
    Unauthorized,
    RetryableError,
    Error { message: String },
}

/// Why a source that *did* answer had no window to draw, when it can say.
///
/// Codex's live usage endpoint reports enforced windows exhaustively and its
/// `rate_limit` object carries `allowed` / `limit_reached`, so an answer
/// without any window is a fact — "used up for this period" or "nothing
/// enforced right now" — not a read failure. Two other sources can also say
/// why: OpenCode Go's usage endpoint returns 403 for a key without the Go
/// plan, and Claude Code answers `rate_limits_available: false` for API key /
/// Bedrock / Vertex logins. Both are definitive, so the card explains the
/// account instead of offering a retry. Every other source (session JSONL,
/// on-disk cache, the other CLI-backed products) cannot tell those apart from
/// "that product has no data here", so it leaves this `None` and the card
/// stays neutral rather than guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RateLimitEmptyReason {
    /// `limit_reached == true`: this period's quota is consumed.
    LimitReached,
    /// The endpoint answered without enforcing any window.
    NoWindow,
    /// The account does not own the subscription the endpoint meters
    /// (OpenCode Go without the Go plan).
    NotEntitled,
    /// The session's login method has plan windows that genuinely do not apply
    /// (Claude Code with an API key, Bedrock or Vertex).
    SessionWithoutPlanLimits,
}

/// Which kind of channel produced what the card shows. Mirrors macOS
/// `ProviderRateLimit.Source`; absent means the producer did not say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RateLimitSource {
    /// This refresh's live read: the Claude Code probe, the Codex endpoint, or
    /// the CLI reporting `source: "live"`.
    Live,
    /// A stored earlier result: Claude Code's `cachedUsageUtilization`, or the
    /// CLI's own quota cache.
    Cache,
    /// Claude Desktop's `plan-usage-history.json` samples.
    DesktopRecord,
    /// A local log: Codex session JSONL, or the CLI reporting `source: "local"`.
    LocalLog,
}

/// Why the preferred read did not produce the card's numbers, as one of our
/// own fixed reasons (codes shared with macOS `ProviderRateLimit.LiveFailure`,
/// plus the Windows-only Claude Desktop record and Credential Manager cases).
///
/// Only the classification travels: never a child process's stderr, an HTTP
/// body, or the CLI's free-text `message`, any of which can carry paths,
/// account names or credentials. A snapshot from a successful live read has
/// none, so the annotation clears itself on the next good refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum RateLimitLiveFailure {
    // Claude Code probe
    ClaudeBinaryNotFound,
    ClaudeLaunchFailed,
    ClaudeTimedOut,
    /// The process exited, closed stdout, or answered without success.
    ClaudeNoResponse,
    /// Answered, but `rate_limits` held no usable 5h/7d window.
    ClaudeNoWindows,
    // Claude Desktop `plan-usage-history.json` (Windows only)
    /// The newest sample is older than the record's 24-hour limit.
    DesktopRecordStale,
    /// More than one Claude Desktop installation keeps a record.
    DesktopMultipleInstalls,
    /// One record holds samples from more than one organization (`org`).
    /// That alone does not prove a second account — one account switching
    /// between a personal and a Team organization looks the same — so the
    /// reason states only what the record shows.
    DesktopMixedOrganizations,
    /// The record could not be read or did not have the expected shape.
    DesktopRecordUnreadable,
    // Native HTTP provider (Codex)
    CredentialsNotFound,
    CredentialsRejected,
    EndpointTimedOut,
    EndpointUnreachable,
    EndpointHttpStatus { status: u16 },
    EndpointUnrecognized,
    // App-held credentials (ZCode key in Windows Credential Manager)
    CredentialStoreUnavailable,
    // Shared quota CLI
    CliRuntimeMissing,
    CliTimedOut,
    CliFailed,
    CliUnrecognizedOutput,
    CliSchemaMismatch,
    CliNoResult,
    /// The CLI answered from its cache after its live read failed; the reason
    /// exists only as free text, which is never displayed.
    CliReportedFailure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRateLimit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
    /// Channel that produced the shown numbers or, for an empty or retryable
    /// card, the channel whose failure the card reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RateLimitSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_failure: Option<RateLimitLiveFailure>,
    pub provider: RateLimitProvider,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meters: Vec<RateLimitMeter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<RateLimitWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<RateLimitWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_as_of: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub five_hour_not_enforced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_credits_count: Option<u64>,
    /// Absent means "the source did not say" — the UI must not render a verdict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<RateLimitEmptyReason>,
    pub status: RateLimitStatus,
}

impl ProviderRateLimit {
    pub fn empty(provider: RateLimitProvider, status: RateLimitStatus) -> Self {
        Self {
            provider,
            meters: Vec::new(),
            five_hour: None,
            seven_day: None,
            plan_label: None,
            data_as_of: None,
            fetched_at: None,
            source_label: None,
            source: None,
            live_failure: None,
            five_hour_not_enforced: false,
            reset_credits_count: None,
            empty_reason: None,
            status,
        }
    }

    /// An empty card that reports why the read failed and which channel it
    /// tried, never a guess at the reason.
    pub fn failed(
        provider: RateLimitProvider,
        status: RateLimitStatus,
        source: Option<RateLimitSource>,
        failure: RateLimitLiveFailure,
    ) -> Self {
        let mut snapshot = Self::empty(provider, status);
        snapshot.source = source;
        snapshot.live_failure = Some(failure);
        snapshot
    }
}

pub(crate) fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_and_empty_reason_wire_ids_match_the_frontend_contract() {
        // For CLI-backed products the provider id *is* the CLI product id, and
        // the bridge resolves the CLI's answer back through this same string.
        assert_eq!(
            serde_json::from_value::<RateLimitProvider>(serde_json::json!("opencode-go")).unwrap(),
            RateLimitProvider::OpenCodeGo
        );
        assert_eq!(
            serde_json::to_value(RateLimitProvider::OpenCodeGo).unwrap(),
            serde_json::json!("opencode-go")
        );
        for (reason, id) in [
            (RateLimitEmptyReason::NotEntitled, "notEntitled"),
            (
                RateLimitEmptyReason::SessionWithoutPlanLimits,
                "sessionWithoutPlanLimits",
            ),
        ] {
            assert_eq!(serde_json::to_value(reason).unwrap(), serde_json::json!(id));
        }
    }
}
