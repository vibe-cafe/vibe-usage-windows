//! The app's own record of its last ten syncs, kept beside `settings.json`.
//!
//! Approved by the maintainer on 2026-11-09 under the architecture approval
//! gate (一键诊断方案修订二, behaviour B: "默认在本机保留最近 10 次同步摘要").
//! Only the shell can see a 120-second timeout or a CLI that never started,
//! so those land here even when the CLI wrote nothing. Every field is typed:
//! no stdout/stderr, server body, path or key is ever stored — the CLI's text
//! is only *matched* against fixed patterns to pick a category.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "sync-history.json";
const LIMIT: usize = 10;
/// The launcher's own prefix (`cli-bootstrap.mjs` `fail()`): the CLI never ran.
const LAUNCHER_PREFIX: &str = "vibe-usage launcher:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncTrigger {
    /// The adaptive scheduler (including the sync right after start).
    Scheduled,
    /// The panel / tray "立即同步", or a sync after changing settings.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncOutcome {
    Success,
    Failed,
    /// The app ended the CLI at `SYNC_TIMEOUT`.
    TimedOut,
    /// The CLI itself never ran: no runtime, spawn failure, or the launcher
    /// could not resolve / download / install the published package.
    CliStartupFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherStage {
    /// Could not reach the npm registry (network error or its timeout).
    Network,
    /// The registry answered with an error or without a version.
    Registry,
    /// Downloading or verifying the package failed.
    Download,
    /// Unpacking or placing the package failed.
    Install,
    Other,
}

/// `rename_all` renames only the variant tags; `rename_all_fields` makes the
/// fields camelCase too, so the frontend reads `exitCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SyncReason {
    /// No bundled or system runtime, or the launcher script is missing.
    RuntimeUnavailable,
    /// The runtime process could not be started.
    SpawnFailed,
    Launcher {
        stage: LauncherStage,
    },
    /// The server rejected the API key.
    KeyRejected,
    /// `HTTP <status>` from the CLI's upload error; only the number is kept.
    ServerStatus {
        status: u16,
    },
    /// Any other non-zero exit of the CLI.
    CliExit {
        /// Records written before this field was renamed used `exit_code`.
        #[serde(alias = "exit_code")]
        exit_code: i32,
    },
    /// Waiting for the process failed.
    WaitFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRecord {
    /// Epoch milliseconds when the sync started.
    pub started_at: u64,
    pub trigger: SyncTrigger,
    pub outcome: SyncOutcome,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SyncReason>,
}

pub fn path_in(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// Newest first. A missing or unreadable file is simply "no records yet".
pub fn load(file: &Path) -> Vec<SyncRecord> {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<SyncRecord>>(&bytes).ok())
        .map(|mut records| {
            records.truncate(LIMIT);
            records
        })
        .unwrap_or_default()
}

/// Prepend one record and keep the newest ten. Best effort: a write failure
/// must never affect the sync it describes.
pub fn append(file: &Path, record: SyncRecord) {
    let mut records = load(file);
    records.insert(0, record);
    records.truncate(LIMIT);
    if let Ok(data) = serde_json::to_vec_pretty(&records) {
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = vibe_core::config::atomic_write(file, &data);
    }
}

/// The CLI's own fixed hint when the server rejects the key (`src/sync.js`,
/// printed as `✗ <hint>` or, after a partial upload,
/// `✗ 部分完成（已上传 …）: <hint>`), plus the older English markers.
const KEY_REJECTED_MARKERS: [&str; 3] = ["API Key 无效", "Invalid API key", "UNAUTHORIZED"];

/// Category of a finished CLI run that exited non-zero, decided from the
/// run's final error line (the one the status bar shows, see
/// `sync_engine::extract_error_line`) — never from a non-fatal tool warning
/// printed earlier, such as `Cursor usage export skipped (HTTP 429 …)`. The
/// output is only matched against fixed patterns; nothing from it is kept.
pub fn classify_exit(
    exit_code: Option<i32>,
    stderr: &str,
    final_line: &str,
) -> (SyncOutcome, SyncReason) {
    if let Some(rest) = stderr
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(LAUNCHER_PREFIX))
    {
        return (
            SyncOutcome::CliStartupFailed,
            SyncReason::Launcher {
                stage: launcher_stage(rest.trim(), stderr),
            },
        );
    }
    if KEY_REJECTED_MARKERS
        .iter()
        .any(|marker| final_line.contains(marker))
    {
        return (SyncOutcome::Failed, SyncReason::KeyRejected);
    }
    // A status number counts only from an actual failure line: the CLI's
    // `✗ …` (or an older `Error: …`). Without one the status bar falls back
    // to the last line it can find, which may be a tool warning.
    let is_failure_line = final_line.contains('✗') || final_line.contains("Error:");
    if let Some(status) = http_status(final_line).filter(|_| is_failure_line) {
        let reason = if matches!(status, 401 | 403) {
            SyncReason::KeyRejected
        } else {
            SyncReason::ServerStatus { status }
        };
        return (SyncOutcome::Failed, reason);
    }
    (
        SyncOutcome::Failed,
        SyncReason::CliExit {
            exit_code: exit_code.unwrap_or(-1),
        },
    )
}

fn launcher_stage(message: &str, stderr: &str) -> LauncherStage {
    if message.starts_with("registry ") {
        LauncherStage::Registry
    } else if message.starts_with("download failed") || message.starts_with("integrity mismatch") {
        LauncherStage::Download
    } else if message.starts_with("could not install") || message.contains(" did not contain ") {
        LauncherStage::Install
    } else if ["fetch failed", "TimeoutError", "aborted due to timeout"]
        .iter()
        .any(|marker| stderr.contains(marker))
    {
        // Node's fetch error and AbortSignal.timeout(): the registry was not
        // reachable at all (the common case behind a blocked npm registry).
        LauncherStage::Network
    } else {
        LauncherStage::Other
    }
}

/// `HTTP 502: …` (the CLI's upload error shape) → `502`.
fn http_status(text: &str) -> Option<u16> {
    text.match_indices("HTTP ").find_map(|(index, _)| {
        let digits = text.get(index + 5..index + 8)?;
        let next = text[index + 8..].chars().next();
        (digits.bytes().all(|b| b.is_ascii_digit()) && next.map_or(true, |c| !c.is_ascii_digit()))
            .then(|| digits.parse().ok())
            .flatten()
            .filter(|status| (100..=599).contains(status))
    })
}

/// What the newest sync says about the API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyVerdict {
    /// No sync recorded yet.
    NoRecord,
    /// The newest sync succeeded.
    Accepted,
    /// The newest sync had its key rejected by the server.
    Rejected,
    /// Anything else — timeout, CLI never started, network failure, a server
    /// error such as 502, any other failure — says nothing about the key.
    Undetermined,
}

/// Only the newest record counts, and only success or a rejected key is a
/// verdict. Reaching back past a timeout or a CLI that never started to an
/// older result would point a network problem at the key (same rule on
/// macOS, Windows and the CLI).
pub fn last_key_verdict(records: &[SyncRecord]) -> KeyVerdict {
    let Some(newest) = records.first() else {
        return KeyVerdict::NoRecord;
    };
    match (newest.outcome, newest.reason) {
        (SyncOutcome::Success, _) => KeyVerdict::Accepted,
        (SyncOutcome::Failed, Some(SyncReason::KeyRejected)) => KeyVerdict::Rejected,
        _ => KeyVerdict::Undetermined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exactly what `sync_engine::run_cli_sync` does with a failed run.
    fn classify(exit_code: Option<i32>, stdout: &str, stderr: &str) -> (SyncOutcome, SyncReason) {
        let line = crate::services::sync_engine::extract_error_line(if stderr.is_empty() {
            stdout
        } else {
            stderr
        });
        classify_exit(exit_code, stderr, &line)
    }

    const KEY_HINT: &str = "API Key 无效，请运行 `npx @vibe-cafe/vibe-usage init` 重新配置。";
    const CURSOR_429: &str = "Cursor usage export skipped (HTTP 429 Too Many Requests)";

    fn record(started_at: u64, outcome: SyncOutcome, reason: Option<SyncReason>) -> SyncRecord {
        SyncRecord {
            started_at,
            trigger: SyncTrigger::Scheduled,
            outcome,
            duration_ms: 1000,
            reason,
        }
    }

    #[test]
    fn keeps_the_newest_ten_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let file = path_in(dir.path());
        assert!(load(&file).is_empty());
        for at in 0..13 {
            append(&file, record(at, SyncOutcome::Success, None));
        }
        let records = load(&file);
        assert_eq!(records.len(), 10);
        assert_eq!(records[0].started_at, 12);
        assert_eq!(records[9].started_at, 3);
    }

    #[test]
    fn unreadable_file_starts_over_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        let file = path_in(dir.path());
        std::fs::write(&file, b"{not json").unwrap();
        assert!(load(&file).is_empty());
        append(&file, record(1, SyncOutcome::TimedOut, None));
        assert_eq!(load(&file).len(), 1);
    }

    #[test]
    fn launcher_failures_are_startup_failures_by_their_fixed_prefix() {
        let cases = [
            (
                "vibe-usage launcher: TypeError: fetch failed\n    at node:internal/deps/undici:1:1\n  cause: Error: connect ETIMEDOUT 104.16.0.35:443",
                LauncherStage::Network,
            ),
            (
                "vibe-usage launcher: TimeoutError: The operation was aborted due to timeout",
                LauncherStage::Network,
            ),
            (
                "vibe-usage launcher: registry returned 503 for @vibe-cafe/vibe-usage@latest",
                LauncherStage::Registry,
            ),
            (
                "vibe-usage launcher: download failed (404) for @vibe-cafe/vibe-usage@0.14.9",
                LauncherStage::Download,
            ),
            (
                "vibe-usage launcher: integrity mismatch for @vibe-cafe/vibe-usage@0.14.9",
                LauncherStage::Download,
            ),
            (
                r"vibe-usage launcher: could not install @vibe-cafe/vibe-usage@0.14.9: EPERM C:\Users\张 三\AppData",
                LauncherStage::Install,
            ),
            ("vibe-usage launcher: something new", LauncherStage::Other),
        ];
        for (stderr, stage) in cases {
            assert_eq!(
                classify(Some(1), "", stderr),
                (
                    SyncOutcome::CliStartupFailed,
                    SyncReason::Launcher { stage }
                ),
                "{stderr}"
            );
        }
    }

    #[test]
    fn cli_failures_keep_only_a_category_and_a_number() {
        assert_eq!(
            classify(Some(1), "", "Error: Invalid API key"),
            (SyncOutcome::Failed, SyncReason::KeyRejected)
        );
        assert_eq!(
            classify(
                Some(1),
                "",
                "Error: HTTP 401: {\"error\":\"bad key vbu_secret\"}"
            ),
            (SyncOutcome::Failed, SyncReason::KeyRejected)
        );
        assert_eq!(
            classify(
                Some(1),
                "",
                "Error: HTTP 502: <html>Bad gateway for someone@example.com</html>"
            ),
            (
                SyncOutcome::Failed,
                SyncReason::ServerStatus { status: 502 }
            )
        );
        assert_eq!(
            classify(
                Some(3),
                "",
                r"cannot read C:\Users\张 三\.claude\projects\-Users-zhang-Coding-secret\a.jsonl"
            ),
            (SyncOutcome::Failed, SyncReason::CliExit { exit_code: 3 })
        );
        assert_eq!(http_status("HTTP 5023"), None);
        assert_eq!(http_status("see HTTP 404."), Some(404));
    }

    /// The CLI's real output (`src/sync.js`): `failure()` prints `✗ <msg>` to
    /// stderr, after a partial upload prefixed with `部分完成（已上传 …）: `.
    #[test]
    fn the_cli_key_hint_is_recognized_with_or_without_the_partial_prefix() {
        for stderr in [
            format!("✗ {KEY_HINT}"),
            format!("{CURSOR_429}\n✗ 部分完成（已上传 12 buckets · 3 sessions）: {KEY_HINT}"),
            format!("\u{1b}[31m✗\u{1b}[39m {KEY_HINT}"),
        ] {
            assert_eq!(
                classify(Some(1), "", &stderr),
                (SyncOutcome::Failed, SyncReason::KeyRejected),
                "{stderr}"
            );
        }
    }

    #[test]
    fn a_tool_warning_earlier_in_the_run_is_not_the_failure_reason() {
        // Final failure is the upload: its status wins over Cursor's 429.
        assert_eq!(
            classify(
                Some(1),
                "Synced 0 buckets",
                &format!("{CURSOR_429}\n✗ 同步失败: HTTP 502: <html>Bad gateway</html>"),
            ),
            (
                SyncOutcome::Failed,
                SyncReason::ServerStatus { status: 502 }
            )
        );
        // Final failure carries no status: undetermined, never the warning's 429.
        assert_eq!(
            classify(
                Some(1),
                "",
                &format!("{CURSOR_429}\n✗ 同步失败: fetch failed")
            ),
            (SyncOutcome::Failed, SyncReason::CliExit { exit_code: 1 })
        );
        // Only a warning and a crash footer: still not a server status.
        assert_eq!(
            classify(Some(1), "", &format!("{CURSOR_429}\nNode.js v22.23.1")),
            (SyncOutcome::Failed, SyncReason::CliExit { exit_code: 1 })
        );
    }

    #[test]
    fn stored_records_hold_no_free_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = path_in(dir.path());
        let (outcome, reason) = classify(
            Some(1),
            "Synced for 张三 at C:\\Users\\张 三",
            "Error: HTTP 500: token sk-ant-api03-SECRET Bearer eyJhbGciOi.payload.sig",
        );
        append(
            &file,
            SyncRecord {
                started_at: 1,
                trigger: SyncTrigger::Manual,
                outcome,
                duration_ms: 2500,
                reason: Some(reason),
            },
        );
        let text = std::fs::read_to_string(&file).unwrap();
        for secret in ["张", "Users", "sk-ant", "Bearer", "eyJ", "Synced"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert!(text.contains("\"serverStatus\""));
    }

    #[test]
    fn key_verdict_reads_only_the_newest_record() {
        let records = vec![
            record(
                3,
                SyncOutcome::CliStartupFailed,
                Some(SyncReason::Launcher {
                    stage: LauncherStage::Network,
                }),
            ),
            record(2, SyncOutcome::TimedOut, None),
            record(1, SyncOutcome::Failed, Some(SyncReason::KeyRejected)),
            record(0, SyncOutcome::Success, None),
        ];
        // The newest run never reached the server: the older key rejection
        // must not be reported as the latest verdict.
        assert_eq!(last_key_verdict(&records), KeyVerdict::Undetermined);
        assert_eq!(last_key_verdict(&records[1..]), KeyVerdict::Undetermined);
        assert_eq!(last_key_verdict(&records[2..]), KeyVerdict::Rejected);
        assert_eq!(last_key_verdict(&records[3..]), KeyVerdict::Accepted);
        assert_eq!(last_key_verdict(&[]), KeyVerdict::NoRecord);
        assert_eq!(
            last_key_verdict(&[record(
                0,
                SyncOutcome::Failed,
                Some(SyncReason::ServerStatus { status: 502 })
            )]),
            KeyVerdict::Undetermined
        );
        assert_eq!(
            last_key_verdict(&[record(
                0,
                SyncOutcome::Failed,
                Some(SyncReason::CliExit { exit_code: 3 })
            )]),
            KeyVerdict::Undetermined
        );
    }

    #[test]
    fn reasons_serialize_with_camel_case_fields_and_read_old_records() {
        assert_eq!(
            serde_json::to_value(SyncReason::CliExit { exit_code: 3 }).unwrap(),
            serde_json::json!({"code": "cliExit", "exitCode": 3})
        );
        assert_eq!(
            serde_json::to_value(SyncReason::ServerStatus { status: 502 }).unwrap(),
            serde_json::json!({"code": "serverStatus", "status": 502})
        );
        assert_eq!(
            serde_json::to_value(SyncReason::Launcher {
                stage: LauncherStage::Network
            })
            .unwrap(),
            serde_json::json!({"code": "launcher", "stage": "network"})
        );
        // A record written by the first build of this feature.
        let old: SyncReason =
            serde_json::from_value(serde_json::json!({"code": "cliExit", "exit_code": 7})).unwrap();
        assert_eq!(old, SyncReason::CliExit { exit_code: 7 });
        let new: SyncReason =
            serde_json::from_value(serde_json::json!({"code": "cliExit", "exitCode": 7})).unwrap();
        assert_eq!(new, SyncReason::CliExit { exit_code: 7 });
    }
}
