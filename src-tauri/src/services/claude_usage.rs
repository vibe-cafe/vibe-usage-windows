//! Claude quota reader using Claude Code's inert stdio control protocol.
//! It sends no prompt, exposes no tools, loads no MCP servers, and persists no
//! session. Claude's own on-disk cache is the offline/no-binary fallback.

use chrono::DateTime;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use vibe_core::claude_code::{ClaudeLaunch, LaunchOrigin, LaunchVia};
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

/// The app's bundled `node.exe`, used to run a JavaScript shim entry only when
/// neither the shim's directory nor PATH has a Node (see
/// `vibe_core::claude_code::discover_with_fallback_node`).
static BUNDLED_NODE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Called once at startup with `sync_engine::bundled_node`.
pub fn set_bundled_node(path: Option<PathBuf>) {
    let _ = BUNDLED_NODE.set(path);
}

/// Grace period for a child that already stopped answering to exit on its
/// own, so its exit code can be reported, before the tree is terminated.
const EXIT_GRACE: Duration = Duration::from_millis(500);
const MAX_CANDIDATES: usize = 3;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// What the most recent real read did, for the copyable diagnostics summary
/// (一键诊断 1b). Typed fields only: fixed origin templates instead of paths,
/// step timings, the process exit code and which answer fields were present.
/// Claude Code's own output (stderr, answer values) is never kept.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRecord {
    /// Epoch milliseconds.
    pub at: u64,
    pub desktop: DesktopStep,
    /// Candidates discovered before the per-refresh limit was applied.
    pub candidates_found: usize,
    pub attempts: Vec<ProbeAttempt>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DesktopStep {
    /// No Claude Desktop record: the live read ran.
    NoRecord,
    /// A Desktop record decided the card; no Claude process was started.
    Used {
        #[serde(skip_serializing_if = "Option::is_none")]
        failure: Option<RateLimitLiveFailure>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeAttempt {
    pub origin: LaunchOrigin,
    pub via: LaunchVia,
    /// From start until Claude Code answered `initialize`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialize_ms: Option<u64>,
    /// From sending `get_usage` until the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get_usage_ms: Option<u64>,
    /// `None` when this candidate produced the card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<RateLimitLiveFailure>,
    pub exit: ProcessExit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<AnswerFields>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProcessExit {
    NotStarted,
    /// Exited on its own; `code` is absent when Windows reported none.
    Exited {
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
    },
    /// Still running when the app ended it (timeout, or after the answer).
    EndedByApp,
}

/// Which fields the `get_usage` answer carried — presence only, no values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limits_available: Option<bool>,
    pub five_hour: bool,
    pub seven_day: bool,
    pub plan: bool,
    pub error: bool,
}

impl AnswerFields {
    fn of(root: &Value) -> Self {
        let window = |key: &str| {
            root.get("rate_limits")
                .and_then(|limits| limits.get(key))
                .is_some_and(|value| !value.is_null())
        };
        Self {
            rate_limits_available: root.get("rate_limits_available").and_then(Value::as_bool),
            five_hour: window("five_hour"),
            seven_day: window("seven_day"),
            plan: root
                .get("subscription_type")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()),
            error: root.get("error").is_some_and(|value| !value.is_null()),
        }
    }
}

/// One discovered candidate at the moment diagnostics were copied.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateNow {
    pub origin: LaunchOrigin,
    pub via: LaunchVia,
    /// `claude --version`, reduced to its version number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

static LAST_PROBE: Mutex<Option<ProbeRecord>> = Mutex::new(None);

fn remember(record: ProbeRecord) {
    *LAST_PROBE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(record);
}

pub fn last_probe() -> Option<ProbeRecord> {
    LAST_PROBE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn discover_candidates(environment: &DiscoveryEnvironment) -> Vec<ClaudeLaunch> {
    let override_bin = std::env::var_os("VIBE_USAGE_CLAUDE_BIN")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    vibe_core::claude_code::discover_with_fallback_node(
        environment,
        override_bin,
        BUNDLED_NODE.get().and_then(Option::as_deref),
    )
}

/// Desktop record first, then a live Claude Code read; the order is unchanged.
/// The error is the fixed reason of the last candidate tried (as on macOS);
/// the child's stderr and launch-error text are never kept.
pub async fn fetch() -> Result<ProviderRateLimit, RateLimitLiveFailure> {
    let at = (now_epoch() * 1000.0) as u64;
    let environment = DiscoveryEnvironment::live();
    // Desktop observations are local-only and must not be confused with a
    // separately configured CLI account. No Claude process is started here.
    if let Some(snapshot) =
        vibe_core::rate_limit::claude_desktop::read(&environment, now_epoch())
    {
        remember(ProbeRecord {
            at,
            desktop: DesktopStep::Used {
                failure: snapshot.live_failure,
            },
            candidates_found: 0,
            attempts: Vec::new(),
        });
        return Ok(snapshot);
    }
    let candidates = discover_candidates(&environment);
    let mut record = ProbeRecord {
        at,
        desktop: DesktopStep::NoRecord,
        candidates_found: candidates.len(),
        attempts: Vec::new(),
    };
    if candidates.is_empty() {
        remember(record);
        return Err(RateLimitLiveFailure::ClaudeBinaryNotFound);
    }
    let mut last_error = RateLimitLiveFailure::ClaudeNoResponse;
    for candidate in candidates.into_iter().take(MAX_CANDIDATES) {
        let mut attempt = ProbeAttempt {
            origin: candidate.origin,
            via: candidate.via,
            initialize_ms: None,
            get_usage_ms: None,
            failure: None,
            exit: ProcessExit::NotStarted,
            answer: None,
        };
        let result = run_probe(&candidate, &mut attempt).await;
        attempt.failure = result.as_ref().err().copied();
        record.attempts.push(attempt);
        match result {
            Ok(snapshot) => {
                remember(record);
                return Ok(snapshot);
            }
            Err(error) => {
                log::debug!(
                    "Claude quota probe {} failed: {error:?}",
                    candidate.program.display()
                );
                last_error = error;
            }
        }
    }
    remember(record);
    Err(last_error)
}

/// The candidates a read would try right now, each with its version. Runs
/// `--version` only when the user copies diagnostics, never during refresh.
pub async fn candidates_with_versions() -> Vec<CandidateNow> {
    let environment = DiscoveryEnvironment::live();
    let candidates: Vec<_> = discover_candidates(&environment)
        .into_iter()
        .take(MAX_CANDIDATES)
        .collect();
    let mut tasks = tokio::task::JoinSet::new();
    for (index, candidate) in candidates.iter().cloned().enumerate() {
        tasks.spawn(async move { (index, version_of(&candidate).await) });
    }
    let mut versions = vec![None; candidates.len()];
    while let Some(Ok((index, version))) = tasks.join_next().await {
        versions[index] = version;
    }
    candidates
        .into_iter()
        .zip(versions)
        .map(|(candidate, version)| CandidateNow {
            origin: candidate.origin,
            via: candidate.via,
            version,
        })
        .collect()
}

async fn version_of(candidate: &ClaudeLaunch) -> Option<String> {
    let mut command = tokio::process::Command::new(&candidate.program);
    if let Some(script) = &candidate.script {
        command.arg(script);
    }
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    crate::process_utils::hide_tokio_command_window(&mut command);
    let output = tokio::time::timeout(VERSION_TIMEOUT, crate::process_lifecycle::output(&mut command))
        .await
        .ok()?
        .ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// `2.1.319 (Claude Code)` → `2.1.319`. Only digits and dots survive, so the
/// rest of the output can never reach the report.
fn parse_version(output: &str) -> Option<String> {
    output
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|token| token.trim_matches('.'))
        .find(|token| {
            token.contains('.')
                && token.len() <= 20
                && token.split('.').all(|part| !part.is_empty())
        })
        .map(str::to_string)
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

async fn run_probe(
    candidate: &ClaudeLaunch,
    attempt: &mut ProbeAttempt,
) -> Result<ProviderRateLimit, RateLimitLiveFailure> {
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
    let mut steps = Steps::new();
    let result =
        tokio::time::timeout(CANDIDATE_TIMEOUT, exchange(stdin, stdout, &mut steps)).await;
    attempt.initialize_ms = steps.initialize.map(millis);
    attempt.get_usage_ms = steps.get_usage.map(millis);
    let payload = match result {
        Ok(Ok(payload)) => payload,
        Ok(Err(error)) => {
            attempt.exit = finish(&mut child, EXIT_GRACE).await;
            return Err(error);
        }
        Err(_) => {
            crate::process_utils::kill_child_tree(&mut child);
            attempt.exit = ProcessExit::EndedByApp;
            return Err(RateLimitLiveFailure::ClaudeTimedOut);
        }
    };
    attempt.answer = Some(AnswerFields::of(&payload));

    if let Some(snapshot) = snapshot_from_payload(&payload, now_epoch()) {
        // stdin was closed by `exchange`; give the child a short graceful-exit
        // window, then terminate the whole tree so no helper survives a refresh.
        attempt.exit = finish(&mut child, Duration::from_secs(1)).await;
        return Ok(snapshot);
    }
    attempt.exit = finish(&mut child, EXIT_GRACE).await;
    Err(RateLimitLiveFailure::ClaudeNoWindows)
}

/// Let the child exit on its own within `grace` (so its code is known), else
/// terminate the whole tree.
async fn finish(child: &mut tokio::process::Child, grace: Duration) -> ProcessExit {
    match tokio::time::timeout(grace, child.wait()).await {
        Ok(Ok(status)) => ProcessExit::Exited {
            code: status.code(),
        },
        _ => {
            crate::process_utils::kill_child_tree(child);
            ProcessExit::EndedByApp
        }
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

/// Step timings of one exchange; filled while it runs, so they survive the
/// exchange being cut off by the candidate timeout.
struct Steps {
    started: Instant,
    usage_sent: Option<Instant>,
    initialize: Option<Duration>,
    get_usage: Option<Duration>,
}

impl Steps {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            usage_sent: None,
            initialize: None,
            get_usage: None,
        }
    }
}

/// Every way the exchange can end without a payload is "no response": the
/// child stopped reading, closed stdout, or answered without success. Which
/// one exactly lives only in its stderr, which is never read.
async fn exchange(
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    steps: &mut Steps,
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
                steps.initialize = Some(steps.started.elapsed());
                steps.usage_sent = Some(Instant::now());
                send(&mut stdin, &usage_request())
                    .await
                    .map_err(|_| NO_RESPONSE)?;
            }
            Some("vibe-usage") => {
                steps.get_usage = steps.usage_sent.map(|sent| sent.elapsed());
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

    /// The coordinator stores whole snapshots: a cache fallback replaces the
    /// live numbers together with its `source` and `liveFailure`, and the next
    /// live success replaces all three again — no annotation outlives the
    /// read that produced it.
    #[test]
    fn coordinator_replaces_numbers_source_and_failure_together() {
        let now = 1_800_000_000.0;
        let live = |five_hour: f64| {
            snapshot_from_payload(
                &json!({
                    "subscription_type": "pro",
                    "rate_limits": {"five_hour": {"utilization": five_hour, "resets_at": "2030-01-01T01:00:00Z"}}
                }),
                now,
            )
            .unwrap()
        };
        let mut cache = crate::state::RateLimitCache::default();
        let generation = cache.zcode_generation();
        let shown = |cache: &crate::state::RateLimitCache| {
            cache.snapshots[&RateLimitProvider::ClaudeCode].0.clone()
        };

        cache.store(live(14.0), generation);
        let first = shown(&cache);
        assert_eq!(first.source, Some(RateLimitSource::Live));
        assert_eq!(first.live_failure, None);
        assert_eq!(first.five_hour.unwrap().utilization, 14.0);

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        std::fs::write(
            &file,
            json!({"cachedUsageUtilization": {
                "fetchedAtMs": (now - 600.0) * 1000.0,
                "utilization": {"seven_day": {"utilization": 47, "resets_at": "2030-01-05T00:00:00Z"}}
            }})
            .to_string(),
        )
        .unwrap();
        let fallback = after_live_failure(
            RateLimitLiveFailure::ClaudeTimedOut,
            cached_snapshot_from(&file, now),
        );
        cache.store(fallback, generation);
        let second = shown(&cache);
        assert_eq!(second.source, Some(RateLimitSource::Cache));
        assert_eq!(second.live_failure, Some(RateLimitLiveFailure::ClaudeTimedOut));
        assert!(second.five_hour.is_none(), "the live 5h number must not linger");
        assert_eq!(second.seven_day.unwrap().utilization, 47.0);
        assert_eq!(second.plan_label, None);

        cache.store(live(20.0), generation);
        let third = shown(&cache);
        assert_eq!(third.source, Some(RateLimitSource::Live));
        assert_eq!(third.live_failure, None, "a good read clears the failure");
        assert_eq!(third.five_hour.unwrap().utilization, 20.0);
        assert!(third.seven_day.is_none(), "the cached 7d number must not linger");
        assert_eq!(third.plan_label.as_deref(), Some("Pro"));
    }

    #[test]
    fn version_output_is_reduced_to_its_number() {
        assert_eq!(parse_version("2.1.319 (Claude Code)\n").as_deref(), Some("2.1.319"));
        assert_eq!(parse_version("claude 2.0.5").as_deref(), Some("2.0.5"));
        assert_eq!(parse_version("Error: C:\\Users\\张 三\\x"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("v.1."), None);
    }

    #[test]
    fn answer_fields_record_presence_not_values() {
        let fields = AnswerFields::of(&json!({
            "rate_limits_available": true,
            "subscription_type": "max",
            "rate_limits": {"five_hour": null, "seven_day": {"utilization": 47}},
            "error": null
        }));
        assert_eq!(
            fields,
            AnswerFields {
                rate_limits_available: Some(true),
                five_hour: false,
                seven_day: true,
                plan: true,
                error: false,
            }
        );
        let text = serde_json::to_string(&fields).unwrap();
        assert!(!text.contains("max") && !text.contains("47"));
    }
}
