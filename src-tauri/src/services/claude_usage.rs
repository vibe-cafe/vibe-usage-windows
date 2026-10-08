//! Claude quota reader using Claude Code's inert stdio control protocol.
//! It sends no prompt, exposes no tools, loads no MCP servers, and persists no
//! session. Claude's own on-disk cache is the offline/no-binary fallback.

use chrono::DateTime;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use vibe_core::{
    ProviderRateLimit, RateLimitEmptyReason, RateLimitProvider, RateLimitStatus, RateLimitWindow,
};

const CANDIDATE_TIMEOUT: Duration = Duration::from_secs(8);
const FIVE_HOURS: f64 = 5.0 * 3600.0;
const SEVEN_DAYS: f64 = 7.0 * 86_400.0;

pub async fn fetch() -> Result<ProviderRateLimit, String> {
    // Desktop observations are local-only and must not be confused with a
    // separately configured CLI account. No Claude process is started here.
    if let Some(snapshot) = vibe_core::rate_limit::claude_desktop::read(
        &vibe_core::quota_product::DiscoveryEnvironment::live(), now_epoch(),
    ) { return Ok(snapshot); }
    let candidates = discover_binaries();
    if candidates.is_empty() {
        return Err("未找到 Claude Code".into());
    }
    let mut last_error = "Claude 未返回订阅配额".to_string();
    for candidate in candidates.into_iter().take(3) {
        match run_probe(&candidate).await {
            Ok(snapshot) => return Ok(snapshot),
            Err(error) => {
                log::debug!("Claude quota probe {} failed: {error}", candidate.display());
                last_error = error;
            }
        }
    }
    Err(last_error)
}

pub fn cached_snapshot() -> Option<ProviderRateLimit> {
    cached_snapshot_from(&claude_config_file(), now_epoch())
}

fn discover_binaries() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("VIBE_USAGE_CLAUDE_BIN") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local").join("bin").join("claude.exe"));
        candidates.push(home.join(".claude").join("local").join("claude.exe"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Claude Code")
                .join("claude.exe"),
        );
    }

    // Native installs may add claude.exe to PATH. `where.exe` is invoked with
    // a constant argument; shell parsing is never involved.
    let mut where_cmd = std::process::Command::new("where.exe");
    where_cmd.arg("claude.exe");
    crate::process_utils::hide_command_window(&mut where_cmd);
    if let Ok(output) = where_cmd.output() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            candidates.push(PathBuf::from(line.trim()));
        }
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|path| path.is_file())
        .filter(|path| seen.insert(path.to_string_lossy().to_ascii_lowercase()))
        .collect()
}

async fn run_probe(candidate: &Path) -> Result<ProviderRateLimit, String> {
    let mut command = tokio::process::Command::new(candidate);
    command.args([
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
    ]);
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
        .map_err(|e| format!("无法启动 Claude: {e}"))?;
    let stdin = child.stdin.take().ok_or("无法连接 Claude stdin")?;
    let stdout = child.stdout.take().ok_or("无法连接 Claude stdout")?;
    let result = tokio::time::timeout(CANDIDATE_TIMEOUT, exchange(stdin, stdout)).await;
    let payload = match result {
        Ok(Ok(payload)) => payload,
        Ok(Err(error)) => {
            crate::process_utils::kill_child_tree(&mut child);
            return Err(error);
        }
        Err(_) => {
            crate::process_utils::kill_child_tree(&mut child);
            return Err("Claude 配额读取超时".into());
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
    Err("Claude 未返回可用配额".into())
}

async fn exchange(
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
) -> Result<Value, String> {
    send(
        &mut stdin,
        &json!({"type":"control_request","request_id":"vibe-init","request":{"subtype":"initialize"}}),
    )
    .await?;
    let mut lines = BufReader::new(stdout).lines();
    let mut sent_usage = false;
    while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
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
                send(
                    &mut stdin,
                    &json!({"type":"control_request","request_id":"vibe-usage","request":{"subtype":"get_usage"}}),
                )
                .await?;
            }
            Some("vibe-usage") => {
                stdin.shutdown().await.map_err(|e| e.to_string())?;
                if response.get("subtype").and_then(Value::as_str) != Some("success") {
                    return Err("Claude 配额请求失败".into());
                }
                return response
                    .get("response")
                    .cloned()
                    .ok_or_else(|| "Claude 配额响应为空".into());
            }
            _ => {}
        }
    }
    Err("Claude 进程未返回配额".into())
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
}
