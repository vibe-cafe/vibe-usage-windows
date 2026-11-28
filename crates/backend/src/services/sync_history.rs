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
/// An exception thrown after the CLI's `run()` started is printed without it
/// (#34), so it lands in the CLI's own categories below.
const LAUNCHER_PREFIX: &str = "vibe-usage launcher:";
/// The launcher's one-line notice when `latest` was out of reach and it ran
/// the newest cached CLI instead (`cli-bootstrap.mjs` `fallBackToCache()`).
/// The CLI did run, so the run is judged by the CLI's own result.
const NOTICE_PREFIX: &str = "vibe-usage launcher notice:";

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
    /// could not resolve / download / install / load the published package.
    /// A crash inside the CLI's own run is `Failed`.
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
    /// The launcher could not get `latest` at this stage and ran the newest
    /// cached CLI instead; the outcome above is that CLI's own result. Only
    /// the stage is kept, never the notice's text or the version. Approved by
    /// the maintainer on 2026-11-11 under the architecture approval gate
    /// (`docs/DIAGNOSTICS.md`, 追加批准).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_fallback: Option<LauncherStage>,
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

/// The stage named by the launcher's cached-CLI notice (`stage=network;`),
/// if the run had one. Only the fixed word is read; an unknown word is
/// `Other` so a newer launcher cannot hide that it fell back.
pub fn launcher_fallback(stderr: &str) -> Option<LauncherStage> {
    let rest = stderr
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(NOTICE_PREFIX))?;
    let word = rest
        .trim_start()
        .strip_prefix("stage=")
        .and_then(|value| value.split(|c: char| !c.is_ascii_lowercase()).next())
        .unwrap_or("");
    Some(match word {
        "network" => LauncherStage::Network,
        "registry" => LauncherStage::Registry,
        "download" => LauncherStage::Download,
        "install" => LauncherStage::Install,
        _ => LauncherStage::Other,
    })
}

/// stderr without the launcher's cached-CLI notice. The status bar and the
/// failure category read only what the CLI itself printed: the notice is not
/// an error, and a run whose only stderr line is the notice still takes its
/// error line from stdout.
pub fn without_launcher_notice(stderr: &str) -> String {
    if !stderr.contains(NOTICE_PREFIX) {
        return stderr.to_string();
    }
    stderr
        .lines()
        .filter(|line| !line.trim_start().starts_with(NOTICE_PREFIX))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
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
    if let Some(stage) = launcher_failure(stderr) {
        return (SyncOutcome::CliStartupFailed, SyncReason::Launcher { stage });
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

/// The launcher's own failure (`vibe-usage launcher: …`, the CLI never ran),
/// by the stage its fixed message names. Shared by sync and `doctor`.
pub fn launcher_failure(stderr: &str) -> Option<LauncherStage> {
    let rest = stderr
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(LAUNCHER_PREFIX))?;
    Some(launcher_stage(rest.trim(), stderr))
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
        classify_with_line(exit_code, stdout, stderr).0
    }

    /// The category and the status-bar line, the way `run_cli_sync` gets them.
    fn classify_with_line(
        exit_code: Option<i32>,
        stdout: &str,
        stderr: &str,
    ) -> ((SyncOutcome, SyncReason), String) {
        let stderr = without_launcher_notice(stderr);
        let line = crate::services::sync_engine::extract_error_line(if stderr.is_empty() {
            stdout
        } else {
            &stderr
        });
        (classify_exit(exit_code, &stderr, &line), line)
    }

    /// What `cli-bootstrap.mjs` prints before running a cached CLI.
    const NOTICE: &str = "vibe-usage launcher notice: stage=network; running cached @vibe-cafe/vibe-usage@0.14.11 because latest is unavailable (fetch failed: ETIMEDOUT)";

    const KEY_HINT: &str = "API Key 无效，请运行 `npx @vibe-cafe/vibe-usage init` 重新配置。";
    const CURSOR_429: &str = "Cursor usage export skipped (HTTP 429 Too Many Requests)";

    fn record(started_at: u64, outcome: SyncOutcome, reason: Option<SyncReason>) -> SyncRecord {
        SyncRecord {
            started_at,
            trigger: SyncTrigger::Scheduled,
            outcome,
            duration_ms: 1000,
            reason,
            cli_fallback: None,
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

    /// The launcher ran a cached CLI because npm was unreachable: the CLI did
    /// start, so a failure is the CLI's own, never a startup failure, and the
    /// notice is never what the status bar shows.
    #[test]
    fn a_cached_cli_run_is_judged_by_the_cli_not_the_launcher() {
        assert_eq!(
            classify_with_line(
                Some(1),
                "",
                &format!("{NOTICE}\n✗ 同步失败: HTTP 502: <html>Bad gateway</html>")
            ),
            (
                (
                    SyncOutcome::Failed,
                    SyncReason::ServerStatus { status: 502 }
                ),
                "✗ 同步失败: HTTP 502: <html>Bad gateway</html>".to_string()
            )
        );
        assert_eq!(
            classify(Some(1), "", &format!("{NOTICE}\n✗ {KEY_HINT}")),
            (SyncOutcome::Failed, SyncReason::KeyRejected)
        );
        // Only the notice on stderr: the error line comes from stdout, as it
        // did before the launcher could fall back.
        assert_eq!(
            classify_with_line(Some(2), "Error: could not read state.json", NOTICE),
            (
                (SyncOutcome::Failed, SyncReason::CliExit { exit_code: 2 }),
                "Error: could not read state.json".to_string()
            )
        );
        // Windows line endings and an indented notice are stripped as well.
        assert_eq!(
            without_launcher_notice(&format!("  {NOTICE}\r\nCursor usage export skipped\r\n")),
            "Cursor usage export skipped"
        );
        assert_eq!(without_launcher_notice(NOTICE), "");
        // No notice: stderr is passed on untouched.
        assert_eq!(without_launcher_notice("  ✗ x\n"), "  ✗ x\n");
    }

    #[test]
    fn the_cached_cli_notice_keeps_only_its_stage() {
        for (word, stage) in [
            ("network", LauncherStage::Network),
            ("registry", LauncherStage::Registry),
            ("download", LauncherStage::Download),
            ("install", LauncherStage::Install),
            ("somethingnew", LauncherStage::Other),
        ] {
            let stderr = format!(
                "vibe-usage launcher notice: stage={word}; running cached @vibe-cafe/vibe-usage@0.14.11 because latest is unavailable (x)\n✗ 同步失败: y"
            );
            assert_eq!(launcher_fallback(&stderr), Some(stage), "{word}");
        }
        assert_eq!(
            launcher_fallback(&format!("Cursor usage export skipped\r\n  {NOTICE}\r\n")),
            Some(LauncherStage::Network)
        );
        assert_eq!(
            launcher_fallback("vibe-usage launcher notice: running cached"),
            Some(LauncherStage::Other)
        );
        // A launcher failure is not a fallback: the CLI never ran.
        assert_eq!(
            launcher_fallback("vibe-usage launcher: TypeError: fetch failed"),
            None
        );
        assert_eq!(launcher_fallback("✗ 同步失败: fetch failed"), None);
    }

    /// #34: the launcher prints an exception from inside the CLI's `run()`
    /// the way Node prints an uncaught error, without its own prefix. The CLI
    /// did start (it may have uploaded already), so this is a CLI failure
    /// with its exit code, never 「CLI 启动阶段失败（引导器其他错误）」.
    #[test]
    fn a_crash_inside_the_cli_is_a_cli_failure_not_a_startup_failure() {
        let crash = "Cursor usage export skipped (HTTP 429 Too Many Requests)\n\
            TypeError: Cannot read properties of undefined (reading 'buckets')\n    \
            at syncOnce (file:///C:/Users/%E5%BC%A0/AppData/Roaming/x/cli-cache/0.14.11/package/src/sync.js:10:5)\n    \
            at async run (file:///C:/x/package/src/index.js:2:3)";
        assert_eq!(
            classify(Some(1), "", crash),
            (SyncOutcome::Failed, SyncReason::CliExit { exit_code: 1 })
        );
        assert_eq!(
            classify(Some(1), "", &format!("{NOTICE}\n{crash}")),
            (SyncOutcome::Failed, SyncReason::CliExit { exit_code: 1 })
        );
        // The launcher's own failure keeps its prefix and stays a startup failure.
        assert_eq!(
            classify(Some(1), "", "vibe-usage launcher: Error: cli failed to load\n    at x"),
            (
                SyncOutcome::CliStartupFailed,
                SyncReason::Launcher {
                    stage: LauncherStage::Other
                }
            )
        );
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
        let stderr_with_notice = &format!(
            "{}\nError: HTTP 500: token sk-ant-api03-SECRET Bearer eyJhbGciOi.payload.sig",
            NOTICE.replace("ETIMEDOUT", r"ETIMEDOUT C:\Users\张 三\AppData")
        );
        let (outcome, reason) = classify(
            Some(1),
            "Synced for 张三 at C:\\Users\\张 三",
            stderr_with_notice,
        );
        append(
            &file,
            SyncRecord {
                started_at: 1,
                trigger: SyncTrigger::Manual,
                outcome,
                duration_ms: 2500,
                reason: Some(reason),
                cli_fallback: launcher_fallback(stderr_with_notice),
            },
        );
        let text = std::fs::read_to_string(&file).unwrap();
        for secret in [
            "张", "Users", "sk-ant", "Bearer", "eyJ", "Synced", "0.14.11", "ETIMEDOUT", "notice",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert!(text.contains("\"serverStatus\""));
        assert!(text.contains("\"cliFallback\": \"network\""));
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
        // Records written before the cached-CLI fallback have no such field.
        let before: SyncRecord = serde_json::from_value(serde_json::json!({
            "startedAt": 1, "trigger": "scheduled", "outcome": "success", "durationMs": 9
        }))
        .unwrap();
        assert_eq!(before.cli_fallback, None);
        let with_fallback = SyncRecord {
            cli_fallback: Some(LauncherStage::Download),
            ..before
        };
        assert_eq!(
            serde_json::to_value(with_fallback).unwrap()["cliFallback"],
            serde_json::json!("download")
        );
        assert!(serde_json::to_value(before)
            .unwrap()
            .get("cliFallback")
            .is_none());
        // A record written by the first build of this feature.
        let old: SyncReason =
            serde_json::from_value(serde_json::json!({"code": "cliExit", "exit_code": 7})).unwrap();
        assert_eq!(old, SyncReason::CliExit { exit_code: 7 });
        let new: SyncReason =
            serde_json::from_value(serde_json::json!({"code": "cliExit", "exitCode": 7})).unwrap();
        assert_eq!(new, SyncReason::CliExit { exit_code: 7 });
    }
}
