//! 「复制诊断信息」 step 2′: the CLI's own `doctor --json`, merged into the
//! app's report.
//!
//! Approved with the one-click diagnostics plan (一键诊断方案修订二, maintainer
//! Hogan, 2026-11-09: "两个 App 并入 `doctor --json`，等 CLI 发布后做"). The CLI
//! side and its JSON contract (`schemaVersion: 1`) shipped in vibe-usage
//! 0.14.11; the contract is `docs/doctor-json.md` in that repository.
//!
//! The command runs only when the user presses the button, with the same
//! runtime, directories and proxy as a sync, and at most `TIMEOUT`. When it
//! cannot answer — the CLI did not start, ran out of time, is too old to have
//! `doctor`, or printed something we do not recognize — the report keeps the
//! app's own half and says so in one fixed phrase.
//!
//! Whitelist only, like the rest of the report: the document is read field by
//! field into the typed structs below and everything else is dropped. Strings
//! survive only in their expected shape (a version is digits and dots, a code
//! is a short lower-case token, a tool name is a short ASCII name); a value of
//! the wrong type or shape is left out rather than copied. The CLI's stderr is
//! only matched against fixed beginnings and never kept.

use crate::services::sync_engine;
use crate::services::sync_history::{self, SyncReason};
use chrono::DateTime;
use serde::Serialize;
use serde_json::{Map, Value};
use std::time::Duration;
use tauri::AppHandle;

/// The plan's budget for the whole command; the CLI's own probes take up to
/// about 12 s, the launcher's registry check up to 10 s with a cached CLI.
pub const TIMEOUT: Duration = Duration::from_secs(30);
const SCHEMA_VERSION: u64 = 1;
/// The CLI keeps ten; anything beyond is not ours to show.
const MAX_SYNCS: usize = 10;
const MAX_TOOLS: usize = 64;
const MAX_WARNING_CODES: usize = 32;
const MAX_MERGED: usize = 8;
/// What the CLI prints on stderr for a command it does not know.
const UNKNOWN_COMMAND: &str = "Unknown command: doctor";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CliDoctor {
    /// The CLI answered with a schemaVersion 1 document.
    Ok { report: DoctorReport },
    /// The CLI never ran: no runtime or launcher script, the process could
    /// not start, or the launcher failed (`vibe-usage launcher:`). Same
    /// reasons as the sync history.
    StartupFailed { reason: SyncReason },
    /// Still running at `TIMEOUT`; the app ended it.
    TimedOut,
    /// A CLI from before `doctor` existed (`Unknown command: doctor`), e.g.
    /// an older cached version run because npm was out of reach.
    NoDoctorCommand,
    /// Exit 0, but stdout is not a JSON object with a `schemaVersion`.
    UnrecognizedOutput,
    /// A contract version this app does not read; only the number is kept.
    UnsupportedSchema { schema_version: u64 },
    /// Any other non-zero exit.
    Failed { exit_code: i32 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<DoctorRuntime>,
    /// `node:sqlite` / `sqlite3-cli` / `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sqlite: Option<String>,
    pub account: DoctorAccount,
    pub network: DoctorNetwork,
    pub daemon: DoctorDaemon,
    pub settings: DoctorSettings,
    pub sync_history: DoctorSyncHistory,
    pub tools: Vec<DoctorTool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorRuntime {
    /// `node` / `bun`.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Account facts the app does not already report itself. The CLI's own Key
/// verdict and device tag are left out: the app reports both from its own
/// records, and two differently derived tags in one report would only
/// confuse.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorAccount {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured: Option<bool>,
    /// `default` / `custom` (the URL itself is never reported).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// `match` / `mismatch` / `unrecorded` / `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_state: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorNetwork {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Probe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_registry: Option<Probe>,
    /// `official` / `custom`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorDaemon {
    /// `launchd` / `systemd` / `taskscheduler`; absent = unsupported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed: Option<bool>,
    /// `npx` / `pinned`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_project: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_roots: Option<u64>,
    /// `set` / `unset`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kiki_start_at: Option<String>,
    /// `set` / `none` / `unset`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magpie_start_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSyncHistory {
    pub available: bool,
    /// Newest first, at most ten.
    pub entries: Vec<DoctorSync>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSync {
    /// Epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    /// `cli` / `daemon` / `mac-app` / `windows-app`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_version: Option<String>,
    /// `success` / `failed` / `partial`; absent = the sync recorded no end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub uploaded: Counts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<DoctorFailure>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorFailure {
    pub code: String,
    /// HTTP status number, only for `server_error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub buckets: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorTool {
    pub id: String,
    pub name: String,
    pub detected: bool,
    pub extra_roots: u64,
    /// Sources a tool without its own parser is uploaded under.
    pub merged_into: Vec<String>,
    pub uploaded: Counts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<DoctorToolSync>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorToolSync {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    pub buckets: u64,
    pub sessions: u64,
    pub skipped: bool,
    pub incomplete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing: Option<Indexing>,
    /// Warning code and how often it occurred, in the CLI's order.
    pub warnings: Vec<WarningCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub server_dropped: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Indexing {
    pub completed: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningCount {
    pub code: String,
    pub count: u64,
}

/// Run `doctor --json` once and read the answer.
pub async fn run(app: &AppHandle) -> CliDoctor {
    let mut command = match sync_engine::doctor_command(app) {
        Ok(command) => command,
        Err(_) => {
            return CliDoctor::StartupFailed {
                reason: SyncReason::RuntimeUnavailable,
            }
        }
    };
    // Dropping the future on timeout drops the child, which is spawned with
    // `kill_on_drop`, the same way the quota bridge ends its CLI.
    match tokio::time::timeout(TIMEOUT, crate::process_lifecycle::output(&mut command)).await {
        Err(_) => CliDoctor::TimedOut,
        Ok(Err(_)) => CliDoctor::StartupFailed {
            reason: SyncReason::SpawnFailed,
        },
        Ok(Ok(output)) => interpret(
            output.status.success(),
            output.status.code(),
            &output.stdout,
            &output.stderr,
        ),
    }
}

/// The finished process, judged by its exit, stdout and the fixed beginnings
/// of its stderr.
fn interpret(success: bool, exit_code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> CliDoctor {
    if success {
        return parse(stdout);
    }
    let stderr = sync_history::without_launcher_notice(&String::from_utf8_lossy(stderr));
    if let Some(stage) = sync_history::launcher_failure(&stderr) {
        return CliDoctor::StartupFailed {
            reason: SyncReason::Launcher { stage },
        };
    }
    if stderr.lines().any(|line| line.trim() == UNKNOWN_COMMAND) {
        return CliDoctor::NoDoctorCommand;
    }
    CliDoctor::Failed {
        exit_code: exit_code.unwrap_or(-1),
    }
}

fn parse(stdout: &[u8]) -> CliDoctor {
    let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(stdout) else {
        return CliDoctor::UnrecognizedOutput;
    };
    let Some(version) = root.get("schemaVersion").and_then(Value::as_u64) else {
        return CliDoctor::UnrecognizedOutput;
    };
    if version != SCHEMA_VERSION {
        return CliDoctor::UnsupportedSchema {
            schema_version: version,
        };
    }
    CliDoctor::Ok {
        report: read_report(&root),
    }
}

fn read_report(root: &Map<String, Value>) -> DoctorReport {
    let account = object(root, "account");
    let network = object(root, "network");
    let daemon = object(root, "daemon");
    let settings = object(root, "settings");
    let history = object(root, "syncHistory");
    DoctorReport {
        cli_version: object(root, "cli").and_then(|cli| version(cli, "version")),
        runtime: object(root, "runtime").and_then(|runtime| {
            Some(DoctorRuntime {
                name: code(runtime, "name")?,
                version: version(runtime, "version"),
            })
        }),
        sqlite: code(root, "sqlite"),
        account: DoctorAccount {
            configured: account.and_then(|a| boolean(a, "configured")),
            server: account.and_then(|a| code(a, "server")),
            upload_state: account.and_then(|a| code(a, "uploadState")),
        },
        network: DoctorNetwork {
            server: network.and_then(|n| object(n, "server")).and_then(probe),
            npm_registry: network.and_then(|n| object(n, "npmRegistry")).and_then(probe),
            registry: network
                .and_then(|n| object(n, "npmRegistry"))
                .and_then(|registry| code(registry, "registry")),
        },
        daemon: DoctorDaemon {
            platform: daemon.and_then(|d| code(d, "platform")),
            installed: daemon.and_then(|d| boolean(d, "installed")),
            mode: daemon.and_then(|d| code(d, "mode")),
        },
        settings: DoctorSettings {
            upload_project: settings.and_then(|s| boolean(s, "uploadProject")),
            extra_roots: settings.and_then(|s| count(s, "extraRoots")),
            kiki_start_at: settings.and_then(|s| code(s, "kikiStartAt")),
            magpie_start_at: settings.and_then(|s| code(s, "magpieStartAt")),
        },
        sync_history: DoctorSyncHistory {
            available: history.and_then(|h| boolean(h, "available")).unwrap_or(false),
            entries: history
                .and_then(|h| h.get("entries"))
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(Value::as_object)
                        .take(MAX_SYNCS)
                        .map(sync_entry)
                        .collect()
                })
                .unwrap_or_default(),
        },
        tools: root
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_object)
                    .filter_map(tool)
                    .take(MAX_TOOLS)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn probe(value: &Map<String, Value>) -> Option<Probe> {
    Some(Probe {
        reachable: boolean(value, "reachable")?,
        latency_ms: count(value, "latencyMs"),
        error: code(value, "error"),
    })
}

fn sync_entry(entry: &Map<String, Value>) -> DoctorSync {
    DoctorSync {
        started_at: timestamp(entry, "startedAt"),
        surface: code(entry, "surface"),
        cli_version: version(entry, "cliVersion"),
        result: code(entry, "result"),
        duration_ms: count(entry, "durationMs"),
        uploaded: counts(object(entry, "uploaded")),
        failure: object(entry, "failure").and_then(|failure| {
            Some(DoctorFailure {
                code: code(failure, "code")?,
                status: count(failure, "status")
                    .filter(|status| (100..=599).contains(status))
                    .map(|status| status as u16),
            })
        }),
    }
}

fn tool(value: &Map<String, Value>) -> Option<DoctorTool> {
    let id = tool_id(value.get("id")?.as_str()?)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .and_then(tool_name)
        .unwrap_or_else(|| id.clone());
    Some(DoctorTool {
        name,
        detected: boolean(value, "detected").unwrap_or(false),
        extra_roots: count(value, "extraRoots").unwrap_or(0),
        merged_into: value
            .get("mergedInto")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .filter_map(tool_id)
                    .take(MAX_MERGED)
                    .collect()
            })
            .unwrap_or_default(),
        uploaded: counts(object(value, "uploaded")),
        last_sync: object(value, "lastSync").map(tool_sync),
        id,
    })
}

fn tool_sync(last: &Map<String, Value>) -> DoctorToolSync {
    DoctorToolSync {
        started_at: timestamp(last, "startedAt"),
        buckets: count(last, "buckets").unwrap_or(0),
        sessions: count(last, "sessions").unwrap_or(0),
        skipped: boolean(last, "skipped").unwrap_or(false),
        incomplete: boolean(last, "incomplete").unwrap_or(false),
        indexing: object(last, "indexing").and_then(|indexing| {
            Some(Indexing {
                completed: count(indexing, "completed")?,
                total: count(indexing, "total")?,
            })
        }),
        warnings: object(last, "warnings")
            .map(|warnings| {
                warnings
                    .iter()
                    .filter_map(|(key, value)| {
                        Some(WarningCount {
                            code: shaped_code(key)?,
                            count: value.as_u64()?,
                        })
                    })
                    .take(MAX_WARNING_CODES)
                    .collect()
            })
            .unwrap_or_default(),
        error: code(last, "error"),
        server_dropped: count(last, "serverDropped").unwrap_or(0),
    }
}

// --- typed, shape-checked readers --------------------------------------------

fn object<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    map.get(key)?.as_object()
}

fn boolean(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key)?.as_bool()
}

fn count(map: &Map<String, Value>, key: &str) -> Option<u64> {
    map.get(key)?.as_u64()
}

fn counts(map: Option<&Map<String, Value>>) -> Counts {
    Counts {
        buckets: map.and_then(|m| count(m, "buckets")).unwrap_or(0),
        sessions: map.and_then(|m| count(m, "sessions")).unwrap_or(0),
    }
}

fn code(map: &Map<String, Value>, key: &str) -> Option<String> {
    shaped_code(map.get(key)?.as_str()?)
}

fn version(map: &Map<String, Value>, key: &str) -> Option<String> {
    crate::services::diagnostics::plain_version(map.get(key)?.as_str()?)
}

/// ISO timestamp → epoch milliseconds.
fn timestamp(map: &Map<String, Value>, key: &str) -> Option<u64> {
    let millis = DateTime::parse_from_rfc3339(map.get(key)?.as_str()?)
        .ok()?
        .timestamp_millis();
    u64::try_from(millis).ok()
}

/// A fixed vocabulary word from the contract (`read_failed`, `node:sqlite`,
/// `sqlite3-cli`, `windows-app`): lower-case ASCII, digits, `_`, `-`, `:`.
/// Unknown words are kept so the report can still show them; anything else
/// is not a code.
fn shaped_code(raw: &str) -> Option<String> {
    let valid = !raw.is_empty()
        && raw.len() <= 40
        && raw.as_bytes()[0].is_ascii_alphanumeric()
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b':'));
    valid.then(|| raw.to_string())
}

/// An upload source id (`claude-code`, `pi-coding-agent`).
fn tool_id(raw: &str) -> Option<String> {
    let valid = !raw.is_empty()
        && raw.len() <= 40
        && raw.as_bytes()[0].is_ascii_alphanumeric()
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    valid.then(|| raw.to_string())
}

/// A tool's display name (`Claude Code`, `GitHub Copilot CLI`, `pi`): short
/// ASCII letters, digits, spaces and `.+()-`.
fn tool_name(raw: &str) -> Option<String> {
    let valid = !raw.is_empty()
        && raw.len() <= 40
        && raw.as_bytes()[0].is_ascii_alphanumeric()
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'+' | b'(' | b')' | b'-'));
    valid.then(|| raw.to_string())
}

/// The example from vibe-usage `docs/doctor-json.md` with Windows values, a
/// tool row of each kind and a few fields the app must drop. Shared with the
/// diagnostics contract fixture.
#[cfg(test)]
pub(crate) const SAMPLE: &str = r#"{
  "schemaVersion": 1,
  "generatedAt": "2026-11-09T06:32:00.000Z",
  "cli": { "version": "0.14.11", "surface": "windows-app" },
  "system": { "platform": "win32", "arch": "x64", "release": "10.0.22631" },
  "runtime": { "name": "node", "version": "22.23.1" },
  "sqlite": "node:sqlite",
  "account": { "configured": true, "server": "default", "uploadState": "match", "lastSyncRejected": false, "deviceTag": "7c2e" },
  "network": {
    "server": { "reachable": true, "latencyMs": 320, "error": null },
    "npmRegistry": { "registry": "official", "reachable": false, "latencyMs": null, "error": "timeout" }
  },
  "daemon": { "platform": "taskscheduler", "installed": false, "mode": null },
  "settings": { "uploadProject": true, "extraRoots": 1, "kikiStartAt": "unset", "magpieStartAt": "none" },
  "syncHistory": {
    "available": true,
    "entries": [
      {
        "startedAt": "2026-11-09T06:27:03.000Z", "endedAt": "2026-11-09T06:27:17.000Z",
        "surface": "windows-app", "cliVersion": "0.14.9", "runtime": { "name": "node", "version": "22.23.1" },
        "result": "success", "durationMs": 14000, "uploaded": { "buckets": 12, "sessions": 3 }, "failure": null
      },
      {
        "startedAt": "2026-11-09T06:12:05.000Z", "endedAt": null,
        "surface": "windows-app", "cliVersion": "0.14.11", "runtime": null,
        "result": null, "durationMs": null, "uploaded": { "buckets": 5, "sessions": 0 }, "failure": null
      },
      {
        "startedAt": "2026-11-09T05:00:00.000Z", "endedAt": null,
        "surface": "cli", "cliVersion": "0.14.10", "runtime": null,
        "result": null, "durationMs": null, "uploaded": { "buckets": 0, "sessions": 0 }, "failure": null
      },
      {
        "startedAt": "2026-11-09T04:00:00.000Z", "endedAt": "2026-11-09T04:00:02.000Z",
        "surface": "daemon", "cliVersion": "0.14.10", "runtime": null,
        "result": "failed", "durationMs": 2000, "uploaded": { "buckets": 0, "sessions": 0 },
        "failure": { "code": "server_error", "status": 502 }
      }
    ]
  },
  "tools": [
    {
      "id": "cursor", "name": "Cursor", "detected": true, "extraRoots": 0,
      "uploaded": { "buckets": 840, "sessions": 0 }, "mergedInto": null,
      "lastSync": {
        "startedAt": "2026-11-09T06:00:00.000Z", "buckets": 0, "sessions": 0, "skipped": true,
        "incomplete": false, "indexing": null, "warnings": { "cursor_token_encrypted": 1 }, "error": null, "serverDropped": 0
      }
    },
    {
      "id": "codex", "name": "Codex CLI", "detected": true, "extraRoots": 1,
      "uploaded": { "buckets": 1204, "sessions": 88 }, "mergedInto": null,
      "lastSync": {
        "startedAt": "2026-11-09T06:00:00.000Z", "buckets": 30, "sessions": 2, "skipped": false,
        "incomplete": false, "indexing": { "completed": 120, "total": 400 }, "warnings": {}, "error": null, "serverDropped": 30
      }
    },
    {
      "id": "cindy", "name": "Cindy", "detected": true, "extraRoots": 0,
      "uploaded": { "buckets": 0, "sessions": 0 }, "mergedInto": ["codex", "pi-coding-agent"], "lastSync": null
    }
  ]
}"#;

#[cfg(test)]
pub(crate) fn sample_report() -> DoctorReport {
    match parse(SAMPLE.as_bytes()) {
        CliDoctor::Ok { report } => report,
        other => panic!("sample must parse: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync_history::LauncherStage;


    fn report(json: &str) -> DoctorReport {
        match parse(json.as_bytes()) {
            CliDoctor::Ok { report } => report,
            other => panic!("expected a report, got {other:?}"),
        }
    }

    #[test]
    fn reads_the_contract_example_field_by_field() {
        let report = report(SAMPLE);
        assert_eq!(report.cli_version.as_deref(), Some("0.14.11"));
        assert_eq!(
            report.runtime,
            Some(DoctorRuntime {
                name: "node".into(),
                version: Some("22.23.1".into())
            })
        );
        assert_eq!(report.sqlite.as_deref(), Some("node:sqlite"));
        assert_eq!(report.account.upload_state.as_deref(), Some("match"));
        assert_eq!(
            report.network.npm_registry,
            Some(Probe {
                reachable: false,
                latency_ms: None,
                error: Some("timeout".into())
            })
        );
        assert_eq!(report.network.registry.as_deref(), Some("official"));
        assert_eq!(report.daemon.platform.as_deref(), Some("taskscheduler"));
        assert_eq!(report.daemon.mode, None);
        assert_eq!(report.settings.magpie_start_at.as_deref(), Some("none"));
        let history = &report.sync_history;
        assert!(history.available);
        assert_eq!(history.entries.len(), 4);
        assert_eq!(history.entries[0].started_at, Some(1_794_205_623_000));
        assert_eq!(history.entries[0].cli_version.as_deref(), Some("0.14.9"));
        assert_eq!(history.entries[1].result, None, "no end recorded");
        assert_eq!(history.entries[1].uploaded.buckets, 5);
        assert_eq!(history.entries[2].surface.as_deref(), Some("cli"));
        assert_eq!(
            history.entries[3].failure,
            Some(DoctorFailure {
                code: "server_error".into(),
                status: Some(502)
            })
        );
        assert_eq!(report.tools.len(), 3);
        let cursor = report.tools[0].last_sync.as_ref().unwrap();
        assert_eq!(
            cursor.warnings,
            vec![WarningCount {
                code: "cursor_token_encrypted".into(),
                count: 1
            }]
        );
        assert_eq!(
            report.tools[1].last_sync.as_ref().unwrap().indexing,
            Some(Indexing {
                completed: 120,
                total: 400
            })
        );
        assert_eq!(report.tools[2].merged_into, ["codex", "pi-coding-agent"]);
        assert!(report.tools[2].last_sync.is_none());
    }

    /// Fields the app reports itself, and the CLI's free-form system fields,
    /// never make it through.
    #[test]
    fn drops_what_the_app_does_not_take() {
        let json = serde_json::to_string(&report(SAMPLE)).unwrap();
        for absent in ["7c2e", "lastSyncRejected", "10.0.22631", "win32", "generatedAt", "endedAt"] {
            assert!(!json.contains(absent), "{absent} leaked into {json}");
        }
    }

    /// Hostile values in every string slot: the wrong shape is dropped, never
    /// copied (the CLI scrubs too; this does not rely on it).
    #[test]
    fn values_of_the_wrong_shape_are_left_out() {
        let hostile = SAMPLE
            .replace("\"0.14.11\"", "\"C:\\\\Users\\\\张 三\\\\x\"")
            .replace("\"node:sqlite\"", "\"someone@example.com\"")
            .replace("\"Codex CLI\"", "\"-Users-zhangsan-Coding-secret-project\"")
            .replace("\"cursor_token_encrypted\"", "\"Bearer eyJhbGciOiJIUzI1NiJ9\"")
            .replace("\"server_error\"", "\"HTTP 502: <html>vbu_test_0123456789abcdef</html>\"")
            .replace("\"match\"", "\"sk-ant-api03-FAKEFAKEFAKE\"")
            .replace("\"taskscheduler\"", "\"http://user:p%40ss@10.0.0.5:8080\"");
        let report = report(&hostile);
        let json = serde_json::to_string(&report).unwrap();
        for secret in [
            "张",
            "Users",
            "someone@example.com",
            "secret-project",
            "Bearer",
            "eyJ",
            "vbu_",
            "sk-ant",
            "10.0.0.5",
            "p%40ss",
            "<html>",
        ] {
            assert!(!json.contains(secret), "{secret} leaked into {json}");
        }
        assert_eq!(report.cli_version, None);
        assert_eq!(report.sqlite, None);
        // A tool whose name is not a plain name keeps its id instead.
        assert_eq!(report.tools[1].name, "codex");
        assert!(report.tools[0].last_sync.as_ref().unwrap().warnings.is_empty());
        assert_eq!(report.sync_history.entries[3].failure, None);
    }

    #[test]
    fn unknown_codes_and_new_fields_are_tolerated() {
        let newer = SAMPLE
            .replace("\"sqlite\": \"node:sqlite\"", "\"sqlite\": \"bun:sqlite\", \"somethingNew\": {\"a\": 1}")
            .replace("\"cursor_token_encrypted\"", "\"brand_new_warning\"");
        let report = report(&newer);
        assert_eq!(report.sqlite.as_deref(), Some("bun:sqlite"));
        assert_eq!(
            report.tools[0].last_sync.as_ref().unwrap().warnings[0].code,
            "brand_new_warning"
        );
    }

    #[test]
    fn caps_entries_and_tools() {
        let mut root: Value = serde_json::from_str(SAMPLE).unwrap();
        let entry = root["syncHistory"]["entries"][0].clone();
        root["syncHistory"]["entries"] = Value::Array(vec![entry; 25]);
        let tool = root["tools"][0].clone();
        root["tools"] = Value::Array(vec![tool; 100]);
        let report = report(&root.to_string());
        assert_eq!(report.sync_history.entries.len(), MAX_SYNCS);
        assert_eq!(report.tools.len(), MAX_TOOLS);
    }

    #[test]
    fn a_document_that_is_not_v1_is_named_not_guessed() {
        assert_eq!(
            parse(br#"{"schemaVersion": 2, "tools": []}"#),
            CliDoctor::UnsupportedSchema { schema_version: 2 }
        );
        for unrecognized in [
            &b"not json"[..],
            b"[]",
            b"{}",
            br#"{"schemaVersion": "1"}"#,
            b"",
        ] {
            assert_eq!(parse(unrecognized), CliDoctor::UnrecognizedOutput);
        }
    }

    #[test]
    fn a_failed_run_is_classified_by_fixed_beginnings_only() {
        let notice = "vibe-usage launcher notice: stage=network; running cached @vibe-cafe/vibe-usage@0.14.9 because latest is unavailable (fetch failed)";
        // A cached CLI from before `doctor`.
        assert_eq!(
            interpret(false, Some(1), b"", format!("{notice}\nUnknown command: doctor\n").as_bytes()),
            CliDoctor::NoDoctorCommand
        );
        // The launcher itself failed: the CLI never ran.
        assert_eq!(
            interpret(
                false,
                Some(1),
                b"",
                b"vibe-usage launcher: TypeError: fetch failed\n    at x\n  cause: Error: connect ETIMEDOUT 104.16.0.35:443"
            ),
            CliDoctor::StartupFailed {
                reason: SyncReason::Launcher {
                    stage: LauncherStage::Network
                }
            }
        );
        // The CLI crashed inside doctor (#34: no launcher prefix).
        assert_eq!(
            interpret(
                false,
                Some(1),
                b"",
                r"TypeError: x is undefined
    at C:\Users\张 三\cli-cache\0.14.11\package\src\doctor.js:1:1"
                    .as_bytes()
            ),
            CliDoctor::Failed { exit_code: 1 }
        );
        // Exit 0 with a fallback notice on stderr is still read from stdout.
        assert!(matches!(
            interpret(true, Some(0), SAMPLE.as_bytes(), notice.as_bytes()),
            CliDoctor::Ok { .. }
        ));
    }

    #[test]
    fn outcomes_serialize_as_tagged_camel_case() {
        assert_eq!(
            serde_json::to_value(CliDoctor::StartupFailed {
                reason: SyncReason::Launcher {
                    stage: LauncherStage::Registry
                }
            })
            .unwrap(),
            serde_json::json!({"kind": "startupFailed", "reason": {"code": "launcher", "stage": "registry"}})
        );
        assert_eq!(
            serde_json::to_value(CliDoctor::UnsupportedSchema { schema_version: 2 }).unwrap(),
            serde_json::json!({"kind": "unsupportedSchema", "schemaVersion": 2})
        );
        assert_eq!(
            serde_json::to_value(CliDoctor::Failed { exit_code: 3 }).unwrap(),
            serde_json::json!({"kind": "failed", "exitCode": 3})
        );
        assert_eq!(
            serde_json::to_value(CliDoctor::TimedOut).unwrap(),
            serde_json::json!({"kind": "timedOut"})
        );
    }
}
