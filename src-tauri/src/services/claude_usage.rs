//! Claude quota reader using Claude Code's inert stdio control protocol.
//! It sends no prompt, exposes no tools, loads no MCP servers, and persists no
//! session. Claude's own on-disk cache is the offline/no-binary fallback.

use chrono::DateTime;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use vibe_core::claude_code::ClaudeLaunch;
use vibe_core::quota_product::DiscoveryEnvironment;
use vibe_core::{
    ProviderRateLimit, RateLimitEmptyReason, RateLimitLiveFailure, RateLimitProvider,
    RateLimitSource, RateLimitStatus, RateLimitWindow,
};

const CANDIDATE_TIMEOUT: Duration = Duration::from_secs(8);
const FIVE_HOURS: f64 = 5.0 * 3600.0;
const SEVEN_DAYS: f64 = 7.0 * 86_400.0;

/// Process-local settings that unset `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`.
/// Removing it from the child environment is not enough: Claude Code re-applies
/// `~/.claude/settings.json` `env` after start, and with the flag set
/// `get_usage` answers `rate_limits: null` on a subscription account. Empty
/// string, not `"0"` (any non-empty value is truthy). Never written to disk.
/// Mirrors macOS `ClaudeUsageProbe.nonessentialTrafficOverrideSettings`.
const NONESSENTIAL_TRAFFIC_OVERRIDE_SETTINGS: &str =
    r#"{"env":{"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC":""}}"#;

/// Mirrors macOS `ClaudeUsageProbe.processArguments`. `--print` is required:
/// Claude Code (2.1.246 and later) exits with "--input-format=stream-json
/// requires --print." before the control protocol starts, so without it every
/// probe fails (vibe-usage-windows#25, vibe-usage-app#35).
const PROBE_ARGS: [&str; 15] = [
    "--print",
    "--safe-mode",
    "--no-session-persistence",
    "--strict-mcp-config",
    "--mcp-config",
    r#"{"mcpServers":{}}"#,
    "--tools",
    "",
    "--output-format",
    "stream-json",
    "--input-format",
    "stream-json",
    "--verbose",
    "--settings",
    NONESSENTIAL_TRAFFIC_OVERRIDE_SETTINGS,
];

/// `get_usage` control request. `skip_behaviors` stops Claude Code from first
/// scanning every local transcript of the last seven days for the `/usage`
/// "what's contributing" analysis; the card never reads it, and on heavy
/// accounts that scan can outlast `CANDIDATE_TIMEOUT`. The quota fields come
/// from the same code path either way. Approved by the maintainer on
/// 2026-11-06 under the architecture approval gate (vibe-usage-windows#25).
fn usage_request() -> Value {
    json!({
        "type": "control_request",
        "request_id": "vibe-usage",
        "request": {"subtype": "get_usage", "skip_behaviors": true}
    })
}

/// Desktop record first, then a live Claude Code read; the order is unchanged.
/// The error is the fixed reason of the last candidate tried (as on macOS);
/// the child's stderr and launch-error text are never kept.
pub async fn fetch() -> Result<ProviderRateLimit, RateLimitLiveFailure> {
    let environment = DiscoveryEnvironment::live();
    // Desktop observations are local-only and must not be confused with a
    // separately configured CLI account. No Claude process is started here.
    if let Some(snapshot) =
        vibe_core::rate_limit::claude_desktop::read(&environment, now_epoch())
    {
        return Ok(snapshot);
    }
    let override_bin = std::env::var_os("VIBE_USAGE_CLAUDE_BIN")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let candidates = vibe_core::claude_code::discover(&environment, override_bin);
    if candidates.is_empty() {
        return Err(RateLimitLiveFailure::ClaudeBinaryNotFound);
    }
    let mut last_error = RateLimitLiveFailure::ClaudeNoResponse;
    for candidate in candidates.into_iter().take(3) {
        match run_probe(&candidate).await {
            Ok(snapshot) => return Ok(snapshot),
            Err(error) => {
                log::debug!(
                    "Claude quota probe {} failed: {error:?}",
                    candidate.program.display()
                );
                last_error = error;
            }
        }
    }
    Err(last_error)
}

pub fn cached_snapshot() -> Option<ProviderRateLimit> {
    cached_snapshot_from(&claude_config_file(), now_epoch())
}

/// What the card shows after the live read failed: Claude Code's own cache
/// when it has a usable entry, annotated with why live failed; otherwise the
/// failure itself. No program at all is not something a retry fixes, so it
/// stays an empty state; every other failure is retryable, as on macOS.
pub fn after_live_failure(
    failure: RateLimitLiveFailure,
    cached: Option<ProviderRateLimit>,
) -> ProviderRateLimit {
    if let Some(mut snapshot) = cached {
        snapshot.live_failure = Some(failure);
        return snapshot;
    }
    if failure == RateLimitLiveFailure::ClaudeBinaryNotFound {
        return ProviderRateLimit::failed(
            RateLimitProvider::ClaudeCode,
            RateLimitStatus::NoData,
            None,
            failure,
        );
    }
    ProviderRateLimit::failed(
        RateLimitProvider::ClaudeCode,
        RateLimitStatus::RetryableError,
        Some(RateLimitSource::Live),
        failure,
    )
}

async fn run_probe(candidate: &ClaudeLaunch) -> Result<ProviderRateLimit, RateLimitLiveFailure> {
    // An npm shim resolved to a JavaScript entry runs as `node <entry> ...`,
    // never through cmd.exe (see `vibe_core::claude_code`).
    let mut command = tokio::process::Command::new(&candidate.program);
    if let Some(script) = &candidate.script {
        command.arg(script);
    }
    command.args(PROBE_ARGS);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .env_remove("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_CODE_CHILD_SESSION")
        .env_remove("CLAUDE_PID");
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    crate::process_utils::hide_tokio_command_window(&mut command);

    let (mut child, _process_guard) = crate::process_lifecycle::spawn(&mut command)
        .map_err(|_| RateLimitLiveFailure::ClaudeLaunchFailed)?;
    let stdin = child
        .stdin
        .take()
        .ok_or(RateLimitLiveFailure::ClaudeLaunchFailed)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(RateLimitLiveFailure::ClaudeLaunchFailed)?;
    let result = tokio::time::timeout(CANDIDATE_TIMEOUT, exchange(stdin, stdout)).await;
    let payload = match result {
        Ok(Ok(payload)) => payload,
        Ok(Err(error)) => {
            crate::process_utils::kill_child_tree(&mut child);
            return Err(error);
        }
        Err(_) => {
            crate::process_utils::kill_child_tree(&mut child);
            return Err(RateLimitLiveFailure::ClaudeTimedOut);
        }
    };

    if let Some(snapshot) = snapshot_from_payload(&payload, now_epoch()) {
        // stdin was closed by `exchange`; give the child a short graceful-exit
        // window, then terminate the whole tree so no helper survives a refresh.
        if tokio::time::timeout(Duration::from_secs(1), child.wait())
            .await
            .is_err()
        {
            crate::process_utils::kill_child_tree(&mut child);
        }
        return Ok(snapshot);
    }
    crate::process_utils::kill_child_tree(&mut child);
    Err(RateLimitLiveFailure::ClaudeNoWindows)
}

/// Every way the exchange can end without a payload is "no response": the
/// child stopped reading, closed stdout, or answered without success. Which
/// one exactly lives only in its stderr, which is never read.
async fn exchange(
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
) -> Result<Value, RateLimitLiveFailure> {
    const NO_RESPONSE: RateLimitLiveFailure = RateLimitLiveFailure::ClaudeNoResponse;
    send(
        &mut stdin,
        &json!({"type":"control_request","request_id":"vibe-init","request":{"subtype":"initialize"}}),
    )
    .await
    .map_err(|_| NO_RESPONSE)?;
    let mut lines = BufReader::new(stdout).lines();
    let mut sent_usage = false;
    while let Some(line) = lines.next_line().await.map_err(|_| NO_RESPONSE)? {
        let Ok(object) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if object.get("type").and_then(Value::as_str) != Some("control_response") {
            continue;
        }
        let Some(response) = object.get("response") else {
            continue;
        };
        match response.get("request_id").and_then(Value::as_str) {
            Some("vibe-init") if !sent_usage => {
                sent_usage = true;
                send(&mut stdin, &usage_request())
                    .await
                    .map_err(|_| NO_RESPONSE)?;
            }
            Some("vibe-usage") => {
                stdin.shutdown().await.map_err(|_| NO_RESPONSE)?;
                if response.get("subtype").and_then(Value::as_str) != Some("success") {
                    return Err(NO_RESPONSE);
                }
                return response.get("response").cloned().ok_or(NO_RESPONSE);
            }
            _ => {}
        }
    }
    Err(NO_RESPONSE)
}

async fn send(stdin: &mut tokio::process::ChildStdin, value: &Value) -> Result<(), String> {
    let mut data = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    data.push(b'\n');
    stdin.write_all(&data).await.map_err(|e| e.to_string())
}

/// Claude's answer for one probe payload.
///
/// `rate_limits_available: false` is Claude saying this login method (API key,
/// Bedrock, Vertex) has no plan windows at all. That is definitive, so it must
/// not be collapsed into "no data here": the card explains the account instead
/// of looking like a missing install and offering a retry that cannot help.
fn snapshot_from_payload(root: &Value, now: f64) -> Option<ProviderRateLimit> {
    if root
        .get("rate_limits_available")
        .and_then(Value::as_bool)
        == Some(false)
    {
        let mut snapshot =
            ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::NoData);
        snapshot.empty_reason = Some(RateLimitEmptyReason::SessionWithoutPlanLimits);
        snapshot.source = Some(RateLimitSource::Live);
        return Some(snapshot);
    }
    parse_payload(root, now)
}

fn parse_payload(root: &Value, now: f64) -> Option<ProviderRateLimit> {
    let limits = root.get("rate_limits")?;
    let five_hour = parse_window(limits.get("five_hour"), FIVE_HOURS, now);
    let seven_day = parse_window(limits.get("seven_day"), SEVEN_DAYS, now);
    if five_hour.is_none() && seven_day.is_none() {
        return None;
    }
    Some(ProviderRateLimit {
        provider: RateLimitProvider::ClaudeCode,
        meters: Vec::new(),
        five_hour,
        seven_day,
        plan_label: root
            .get("subscription_type")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(capitalize),
        data_as_of: Some(now),
        fetched_at: Some(now),
        source_label: None,
        source: Some(RateLimitSource::Live),
        live_failure: None,
        five_hour_not_enforced: false,
        reset_credits_count: None,
        // A JSONL/cache snapshot cannot tell "used up" from "nothing here".
        empty_reason: None,
        status: RateLimitStatus::Ok,
    })
}

fn parse_window(raw: Option<&Value>, duration: f64, now: f64) -> Option<RateLimitWindow> {
    let dict = raw?.as_object()?;
    let utilization = dict.get("utilization")?.as_f64()?;
    let resets_at = dict
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(parse_iso_epoch);
    Some(RateLimitWindow {
        utilization,
        resets_at,
        window_duration: resets_at.filter(|at| *at > now).map(|_| duration),
    })
}

fn claude_config_file() -> PathBuf {
    if let Some(custom) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(custom).join(".claude.json");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude.json")
}

fn cached_snapshot_from(path: &Path, now: f64) -> Option<ProviderRateLimit> {
    let root: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let cached = root.get("cachedUsageUtilization")?;
    let fetched_at = cached.get("fetchedAtMs")?.as_f64()? / 1000.0;
    if !(0.0..=SEVEN_DAYS).contains(&(now - fetched_at)) {
        return None;
    }
    let cached_account = cached.get("accountUuid").and_then(Value::as_str);
    let active_account = root
        .pointer("/oauthAccount/accountUuid")
        .and_then(Value::as_str);
    if cached_account.is_some() && active_account.is_some() && cached_account != active_account {
        return None;
    }
    let limits = cached.get("utilization")?;
    let five_hour = parse_cache_window(limits.get("five_hour"), FIVE_HOURS, now);
    let seven_day = parse_cache_window(limits.get("seven_day"), SEVEN_DAYS, now);
    if five_hour.is_none() && seven_day.is_none() {
        return None;
    }
    Some(ProviderRateLimit {
        provider: RateLimitProvider::ClaudeCode,
        meters: Vec::new(),
        five_hour,
        seven_day,
        plan_label: None,
        data_as_of: Some(fetched_at),
        fetched_at: Some(fetched_at),
        source_label: None,
        source: Some(RateLimitSource::Cache),
        live_failure: None,
        five_hour_not_enforced: false,
        reset_credits_count: None,
        // A JSONL/cache snapshot cannot tell "used up" from "nothing here".
        empty_reason: None,
        status: RateLimitStatus::Ok,
    })
}

fn parse_cache_window(raw: Option<&Value>, duration: f64, now: f64) -> Option<RateLimitWindow> {
    let window = parse_window(raw, duration, now)?;
    if window.resets_at.is_some_and(|at| at <= now) {
        return None;
    }
    Some(window)
}

fn parse_iso_epoch(raw: &str) -> Option<f64> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.timestamp_millis() as f64 / 1000.0)
}

fn capitalize(raw: &str) -> String {
    let mut chars = raw.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_live_payload_without_prompt_data() {
        let payload = json!({
            "subscription_type":"max",
            "rate_limits":{"five_hour":{"utilization":14,"resets_at":"2030-01-01T01:00:00Z"},"seven_day":{"utilization":10,"resets_at":"2030-01-05T00:00:00Z"}}
        });
        let result = parse_payload(&payload, 1_800_000_000.0).unwrap();
        assert_eq!(result.plan_label.as_deref(), Some("Max"));
        assert_eq!(result.five_hour.unwrap().utilization, 14.0);
    }

    #[test]
    fn not_applicable_login_reports_the_login_method_instead_of_missing_data() {
        let snapshot =
            snapshot_from_payload(&json!({"rate_limits_available": false}), 1_800_000_000.0)
                .unwrap();
        assert_eq!(snapshot.status, RateLimitStatus::NoData);
        assert_eq!(
            snapshot.empty_reason,
            Some(RateLimitEmptyReason::SessionWithoutPlanLimits)
        );

        // A plan-backed session keeps the ordinary live parsing path.
        let payload = json!({
            "rate_limits_available": true,
            "rate_limits":{"five_hour":{"utilization":14,"resets_at":"2030-01-01T01:00:00Z"}}
        });
        let snapshot = snapshot_from_payload(&payload, 1_800_000_000.0).unwrap();
        assert_eq!(snapshot.status, RateLimitStatus::Ok);
        assert_eq!(snapshot.empty_reason, None);
    }

    /// Without `--print` Claude Code rejects stream-json input and exits before
    /// `initialize`; the argument list must stay in step with macOS.
    #[test]
    fn probe_runs_in_print_mode_with_the_same_arguments_as_macos() {
        assert_eq!(
            PROBE_ARGS,
            [
                "--print",
                "--safe-mode",
                "--no-session-persistence",
                "--strict-mcp-config",
                "--mcp-config",
                r#"{"mcpServers":{}}"#,
                "--tools",
                "",
                "--output-format",
                "stream-json",
                "--input-format",
                "stream-json",
                "--verbose",
                "--settings",
                NONESSENTIAL_TRAFFIC_OVERRIDE_SETTINGS,
            ]
        );
        assert!(!PROBE_ARGS.contains(&"--bare"));
    }

    /// The override unsets exactly one key, with an empty (falsy) string.
    #[test]
    fn settings_override_only_unsets_nonessential_traffic() {
        let index = PROBE_ARGS.iter().position(|a| *a == "--settings").unwrap();
        let settings: Value = serde_json::from_str(PROBE_ARGS[index + 1]).unwrap();
        assert_eq!(
            settings,
            json!({"env": {"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": ""}})
        );
    }

    /// Without `skip_behaviors`, Claude Code scans seven days of transcripts
    /// before it answers; heavy accounts time out (vibe-usage-windows#25).
    #[test]
    fn usage_request_skips_the_transcript_scan() {
        let request = usage_request();
        assert_eq!(request["type"], "control_request");
        assert_eq!(request["request_id"], "vibe-usage");
        assert_eq!(
            request["request"],
            json!({"subtype": "get_usage", "skip_behaviors": true})
        );
        assert!(request["request"]["skip_behaviors"].is_boolean());
    }

    /// With `skip_behaviors` the reply carries `behaviors: null`; windows and
    /// plan are unaffected.
    #[test]
    fn reply_without_behaviors_keeps_windows_and_plan() {
        let payload = json!({
            "subscription_type": "max",
            "rate_limits_available": true,
            "behaviors": null,
            "rate_limits": {
                "five_hour": {"utilization": 3, "resets_at": "2030-01-01T01:00:00Z"},
                "seven_day": {"utilization": 42, "resets_at": "2030-01-05T00:00:00Z"}
            }
        });
        let snapshot = snapshot_from_payload(&payload, 1_800_000_000.0).unwrap();
        assert_eq!(snapshot.status, RateLimitStatus::Ok);
        assert_eq!(snapshot.plan_label.as_deref(), Some("Max"));
        assert_eq!(snapshot.five_hour.unwrap().utilization, 3.0);
        assert_eq!(snapshot.seven_day.unwrap().utilization, 42.0);
    }

    /// Verified Ultra accounts (Max 20x was renamed on 2026-10-30) have no 5h
    /// window. That is an ordinary subscription answer, not "plan limits don't
    /// apply"; Claude Code still reports `subscription_type: "max"` for them.
    #[test]
    fn weekly_only_plan_parses_without_a_five_hour_window() {
        let payload = json!({
            "subscription_type": "max",
            "rate_limits_available": true,
            "rate_limits": {
                "five_hour": null,
                "seven_day": {"utilization": 47, "resets_at": "2030-01-05T00:00:00Z"}
            }
        });
        let snapshot = snapshot_from_payload(&payload, 1_800_000_000.0).unwrap();
        assert_eq!(snapshot.status, RateLimitStatus::Ok);
        assert_eq!(snapshot.empty_reason, None);
        assert!(snapshot.five_hour.is_none());
        assert_eq!(snapshot.seven_day.unwrap().utilization, 47.0);
        assert_eq!(snapshot.plan_label.as_deref(), Some("Max"));
    }

    #[test]
    fn cache_without_a_five_hour_window_keeps_the_weekly_window() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        let now = 1_800_000_000.0;
        let value = json!({
            "oauthAccount": {"accountUuid": "same"},
            "cachedUsageUtilization": {
                "fetchedAtMs": (now - 60.0) * 1000.0,
                "accountUuid": "same",
                "utilization": {
                    "five_hour": null,
                    "seven_day": {"utilization": 47, "resets_at": "2030-01-05T00:00:00Z"}
                }
            }
        });
        std::fs::write(&file, value.to_string()).unwrap();
        let snapshot = cached_snapshot_from(&file, now).unwrap();
        assert!(snapshot.five_hour.is_none());
        assert_eq!(snapshot.seven_day.unwrap().utilization, 47.0);
    }

    #[test]
    fn rejects_stale_or_cross_account_cache() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        let now = 1_800_000_000.0;
        let value = json!({
            "oauthAccount":{"accountUuid":"new"},
            "cachedUsageUtilization":{"fetchedAtMs":now * 1000.0,"accountUuid":"old","utilization":{"five_hour":{"utilization":14,"resets_at":"2030-01-01T01:00:00Z"}}}
        });
        std::fs::write(&file, value.to_string()).unwrap();
        assert!(cached_snapshot_from(&file, now).is_none());
    }

    #[test]
    fn live_failure_keeps_the_cache_and_says_why_live_failed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        let now = 1_800_000_000.0;
        let value = json!({
            "cachedUsageUtilization": {
                "fetchedAtMs": (now - 60.0) * 1000.0,
                "utilization": {"seven_day": {"utilization": 47, "resets_at": "2030-01-05T00:00:00Z"}}
            }
        });
        std::fs::write(&file, value.to_string()).unwrap();
        let cached = cached_snapshot_from(&file, now).unwrap();
        assert_eq!(cached.source, Some(RateLimitSource::Cache));

        let shown = after_live_failure(RateLimitLiveFailure::ClaudeTimedOut, Some(cached));
        assert_eq!(shown.status, RateLimitStatus::Ok);
        assert_eq!(shown.source, Some(RateLimitSource::Cache));
        assert_eq!(shown.live_failure, Some(RateLimitLiveFailure::ClaudeTimedOut));
        assert_eq!(shown.seven_day.unwrap().utilization, 47.0);
    }

    #[test]
    fn live_failure_without_cache_is_retryable_except_for_a_missing_program() {
        for failure in [
            RateLimitLiveFailure::ClaudeLaunchFailed,
            RateLimitLiveFailure::ClaudeTimedOut,
            RateLimitLiveFailure::ClaudeNoResponse,
            RateLimitLiveFailure::ClaudeNoWindows,
        ] {
            let shown = after_live_failure(failure, None);
            assert_eq!(shown.status, RateLimitStatus::RetryableError);
            assert_eq!(shown.source, Some(RateLimitSource::Live));
            assert_eq!(shown.live_failure, Some(failure));
        }
        let missing = after_live_failure(RateLimitLiveFailure::ClaudeBinaryNotFound, None);
        assert_eq!(missing.status, RateLimitStatus::NoData);
        assert_eq!(missing.source, None);
        assert_eq!(
            missing.live_failure,
            Some(RateLimitLiveFailure::ClaudeBinaryNotFound)
        );
    }

    #[test]
    fn live_answers_are_marked_live() {
        let payload = json!({
            "rate_limits":{"five_hour":{"utilization":14,"resets_at":"2030-01-01T01:00:00Z"}}
        });
        let snapshot = snapshot_from_payload(&payload, 1_800_000_000.0).unwrap();
        assert_eq!(snapshot.source, Some(RateLimitSource::Live));
        assert_eq!(snapshot.live_failure, None);
        let not_applicable =
            snapshot_from_payload(&json!({"rate_limits_available": false}), 1_800_000_000.0)
                .unwrap();
        assert_eq!(not_applicable.source, Some(RateLimitSource::Live));
        // Answered, but no usable window: the probe reports "no windows".
        assert!(snapshot_from_payload(&json!({"rate_limits": null}), 1_800_000_000.0).is_none());
    }
}
