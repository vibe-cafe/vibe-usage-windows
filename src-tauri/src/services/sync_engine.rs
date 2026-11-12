//! CLI sync subprocess — port of Services/SyncEngine.swift.
//!
//! Runs `node <resources>/cli-bootstrap.mjs sync`, which resolves
//! `@vibe-cafe/vibe-usage@latest` from the registry, caches it, and runs it with
//! this app's bundled Node — 120s timeout, no console window. The CLI is *not*
//! bundled: a pinned copy rots silently and freezes users out of every CLI fix,
//! which is the policy the macOS app already follows (`@latest` only).
use crate::process_utils;
use crate::services::sync_history::{
    self, LauncherStage, SyncOutcome, SyncReason, SyncRecord, SyncTrigger,
};
use crate::state::{AppCtx, SyncState, SyncStatus};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use vibe_core::runtime::{self, Runtime, RuntimeKind};

pub const SYNC_TIMEOUT: Duration = Duration::from_secs(120);

fn resource_path(app: &AppHandle, rel: &str) -> Option<PathBuf> {
    app.path().resolve(rel, tauri::path::BaseDirectory::Resource).ok()
}

/// The CLI launcher script. It resolves and runs the published CLI, so nothing
/// here depends on a particular CLI version.
pub fn cli_entry(app: &AppHandle) -> Option<PathBuf> {
    let p = resource_path(app, "cli-bootstrap.mjs")?;
    p.is_file().then_some(p)
}

/// The bundled node.exe (Windows release bundles only; absent in dev).
pub fn bundled_node(app: &AppHandle) -> Option<PathBuf> {
    let p = resource_path(app, "node/node.exe")?;
    p.is_file().then_some(p)
}

fn probe_node_version(path: &std::path::Path) -> Option<(u32, u32, u32)> {
    let mut cmd = std::process::Command::new(path);
    cmd.arg("-v").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
    process_utils::hide_command_window(&mut cmd);
    let out = crate::process_lifecycle::output_sync(&mut cmd).ok()?;
    runtime::parse_node_version(&String::from_utf8_lossy(&out.stdout))
}

pub fn detect_runtime(app: &AppHandle) -> Option<Runtime> {
    runtime::detect(bundled_node(app), probe_node_version)
}

/// The runtime's version number for diagnostics (`22.23.1`), nothing else.
pub fn runtime_version(rt: &Runtime) -> Option<String> {
    if rt.kind != RuntimeKind::Bun {
        return probe_node_version(&rt.path).map(|(a, b, c)| format!("{a}.{b}.{c}"));
    }
    let mut cmd = std::process::Command::new(&rt.path);
    cmd.arg("--version").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
    process_utils::hide_command_window(&mut cmd);
    let out = crate::process_lifecycle::output_sync(&mut cmd).ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text.trim();
    (!version.is_empty()
        && version.len() <= 20
        && version.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
    .then(|| version.to_string())
}

/// The node path used for the Claude statusline wrapper command. Falls back
/// to plain "node" (PATH lookup at statusline render time).
pub fn node_for_statusline(app: &AppHandle) -> PathBuf {
    match detect_runtime(app) {
        Some(rt) if rt.kind != RuntimeKind::Bun => rt.path,
        _ => PathBuf::from("node"),
    }
}

fn cli_command(app: &AppHandle, args: &[&str]) -> Result<tokio::process::Command, String> {
    let cli = cli_entry(app).ok_or("未找到 CLI 启动脚本资源")?;
    let rt = detect_runtime(app).ok_or("未检测到可用的 Node.js 运行时，请安装 Node.js 22+")?;
    let cli_dir = cli.parent().ok_or("CLI 启动脚本路径无效")?;
    let cli_file = cli.file_name().ok_or("CLI 启动脚本路径无效")?;

    let mut cmd = tokio::process::Command::new(&rt.path);
    cmd.current_dir(cli_dir)
        .arg(cli_file)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(dir) = rt.path.parent() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}{sep}{path}", dir.display()));
    }
    // The CLI resolves its state file from `VIBE_USAGE_STATE_DIR` alone — it does
    // not follow the config dir — so both have to be set. The Windows build has
    // always kept the two in the same directory, and passing the config dir twice
    // is what keeps a run from writing `~/.vibe-usage/state.json` behind the
    // app's back (which no uninstall would clean up).
    cmd.env(
        "VIBE_USAGE_CONFIG_DIR",
        app.state::<AppCtx>().config.config_dir.clone(),
    );
    cmd.env(
        "VIBE_USAGE_STATE_DIR",
        app.state::<AppCtx>().config.config_dir.clone(),
    );
    cmd.env("VIBE_USAGE_SURFACE", "windows-app");
    cmd.env(
        "VIBE_USAGE_SURFACE_VERSION",
        app.package_info().version.to_string(),
    );
    if crate::state::IS_DEV {
        cmd.env("VIBE_USAGE_DEV", "1");
    }
    if std::env::var("HTTPS_PROXY").is_err() && std::env::var("https_proxy").is_err() {
        if let Some(proxy) = process_utils::system_proxy_url() {
            cmd.env("HTTPS_PROXY", &proxy);
            cmd.env("HTTP_PROXY", &proxy);
        }
    }
    cmd.env("NODE_USE_ENV_PROXY", "1");
    process_utils::hide_tokio_command_window(&mut cmd);
    Ok(cmd)
}

/// Build a command for the typed quota bridge. Arguments are owned by the
/// caller but copied into Command before this function returns.
pub(super) fn quota_command(
    app: &AppHandle,
    args: &[String],
) -> Result<tokio::process::Command, String> {
    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    cli_command(app, &borrowed)
}

/// `doctor --json` for 「复制诊断信息」, with the same runtime, directories,
/// surface and proxy as a sync (`services::cli_doctor`).
pub(super) fn doctor_command(app: &AppHandle) -> Result<tokio::process::Command, String> {
    cli_command(app, &["doctor", "--json"])
}

/// Run a short config command against the same bundled CLI and config directory
/// used by sync.
pub async fn run_config_command(app: &AppHandle, args: &[&str]) -> Result<String, String> {
    let output = tokio::time::timeout(Duration::from_secs(30), crate::process_lifecycle::output(&mut cli_command(app, args)?))
        .await
        .map_err(|_| "CLI 配置操作超时".to_string())?
        .map_err(|e| format!("CLI 配置操作失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        return Ok(stdout);
    }
    let stderr = sync_history::without_launcher_notice(&String::from_utf8_lossy(&output.stderr));
    let message = extract_error_line(&stderr);
    Err(if message.is_empty() {
        format!("CLI 配置操作失败: Exit code {}", output.status.code().unwrap_or(-1))
    } else {
        message
    })
}

fn set_state(app: &AppHandle, update: impl FnOnce(&mut SyncState)) {
    let ctx = app.state::<crate::state::AppCtx>();
    let snapshot = {
        let mut state = ctx.sync_state.lock().unwrap();
        update(&mut state);
        state.clone()
    };
    let _ = app.emit("sync-state", &snapshot);
    crate::tray::update_tray(app);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Request work that must not be dropped if another run is already in flight.
/// Overlapping callers coalesce into a single follow-up after the in-flight
/// run finishes (so a post-config sync still executes).
pub async fn run_with_follow_up<F, Fut>(
    running: &tokio::sync::Mutex<()>,
    pending: &AtomicBool,
    work: F,
) where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    pending.store(true, Ordering::SeqCst);
    let mut guard = match running.try_lock() {
        Ok(g) => Some(g),
        Err(_) => {
            log::info!("sync already running; queued follow-up");
            return;
        }
    };

    loop {
        pending.store(false, Ordering::SeqCst);
        work().await;
        if pending.load(Ordering::SeqCst) {
            log::info!("running queued follow-up sync");
            continue;
        }
        drop(guard.take());
        if !pending.load(Ordering::SeqCst) {
            return;
        }
        match running.try_lock() {
            Ok(g) => guard = Some(g),
            Err(_) => return,
        }
    }
}

/// Run one sync. Concurrent calls queue a follow-up instead of being dropped;
/// the follow-up is recorded with the trigger of the latest request.
pub async fn run_sync(app: AppHandle, trigger: SyncTrigger) {
    let work_app = app.clone();
    let ctx = app.state::<AppCtx>();
    ctx.sync_requested_manually
        .store(trigger == SyncTrigger::Manual, Ordering::SeqCst);
    run_with_follow_up(&ctx.sync_running, &ctx.sync_pending, move || {
        let app = work_app.clone();
        async move {
            run_sync_once(app).await;
        }
    })
    .await;
}

async fn run_sync_once(app: AppHandle) {
    let ctx = app.state::<AppCtx>();

    if !ctx.config.is_configured() {
        return;
    }

    set_state(&app, |s| {
        s.status = SyncStatus::Syncing;
        s.message = None;
    });

    let trigger = if ctx.sync_requested_manually.load(Ordering::SeqCst) {
        SyncTrigger::Manual
    } else {
        SyncTrigger::Scheduled
    };
    let started_at = now_ms();
    let started = std::time::Instant::now();
    let run = run_cli_sync(&app).await;
    // Every outcome is recorded, including the timeout and a CLI that never
    // started, which only the app can see.
    if let Some(dir) = ctx.settings_path.parent() {
        sync_history::append(
            &sync_history::path_in(dir),
            SyncRecord {
                started_at,
                trigger,
                outcome: run.outcome,
                duration_ms: started.elapsed().as_millis() as u64,
                reason: run.reason,
                cli_fallback: run.cli_fallback,
            },
        );
    }

    match run.message {
        Ok(message) => {
            set_state(&app, |s| {
                s.status = SyncStatus::Success;
                s.message = Some(message);
                s.last_sync_at = Some(now_ms());
            });
            // Reset to idle after 3s (mirrors AppState.triggerSync).
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let ctx = app2.state::<AppCtx>();
                let still_success =
                    ctx.sync_state.lock().unwrap().status == SyncStatus::Success;
                if still_success {
                    set_state(&app2, |s| s.status = SyncStatus::Idle);
                }
            });
        }
        Err(message) => {
            set_state(&app, |s| {
                s.status = SyncStatus::Error;
                s.message = Some(message);
            });
        }
    }
}

/// One CLI run: the status line shown in the UI (unchanged wording) plus the
/// typed category kept in the app's sync history.
struct CliRun {
    message: Result<String, String>,
    outcome: SyncOutcome,
    reason: Option<SyncReason>,
    /// The launcher ran a cached CLI because `latest` was out of reach.
    cli_fallback: Option<LauncherStage>,
}

impl CliRun {
    fn failed(message: String, outcome: SyncOutcome, reason: Option<SyncReason>) -> Self {
        Self {
            message: Err(message),
            outcome,
            reason,
            cli_fallback: None,
        }
    }
}

async fn run_cli_sync(app: &AppHandle) -> CliRun {
    let mut cmd = match cli_command(app, &["sync"]) {
        Ok(cmd) => cmd,
        Err(e) => {
            return CliRun::failed(
                format!("同步失败: {e}"),
                SyncOutcome::CliStartupFailed,
                Some(SyncReason::RuntimeUnavailable),
            )
        }
    };

    let (mut child, _process_guard) = match crate::process_lifecycle::spawn(&mut cmd) {
        Ok(spawned) => spawned,
        Err(e) => {
            return CliRun::failed(
                format!("同步失败: {e}"),
                SyncOutcome::CliStartupFailed,
                Some(SyncReason::SpawnFailed),
            )
        }
    };
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    // Read both pipes concurrently with waiting so the child never blocks on
    // a full pipe buffer.
    let io_task = read_both(stdout_pipe, stderr_pipe);

    let combined = tokio::select! {
        res = async {
            let (io, status) = tokio::join!(io_task, child.wait());
            (io, status)
        } => res,
        _ = tokio::time::sleep(SYNC_TIMEOUT) => {
            process_utils::kill_child_tree(&mut child);
            write_timeout_log(app);
            return CliRun::failed("同步超时".into(), SyncOutcome::TimedOut, None);
        }
    };

    let ((stdout, stderr), status) = combined;
    let status = match status {
        Ok(status) => status,
        Err(e) => {
            return CliRun::failed(
                format!("同步失败: {e}"),
                SyncOutcome::Failed,
                Some(SyncReason::WaitFailed),
            )
        }
    };
    let stdout = stdout.trim().to_string();
    let stderr = stderr.trim().to_string();
    write_sync_log(app, &status, &stdout, &stderr);
    // The launcher's notice that it ran a cached CLI is not the CLI's output:
    // only its stage is kept, and nothing below may read it as the run's error.
    let cli_fallback = sync_history::launcher_fallback(&stderr);
    if let Some(stage) = cli_fallback {
        log::info!("latest CLI unavailable ({stage:?}); the launcher ran its newest cached version");
    }
    let stderr = sync_history::without_launcher_notice(&stderr);

    if status.success() {
        // "Synced …" / "No new usage data" both count as success.
        return CliRun {
            message: Ok(if stdout.is_empty() { "同步完成".into() } else { stdout }),
            outcome: SyncOutcome::Success,
            reason: None,
            cli_fallback,
        };
    }
    // The line the status bar shows is the run's final error; the history
    // classifies that line only, never a tool warning printed earlier.
    let line = extract_error_line(if stderr.is_empty() { &stdout } else { &stderr });
    let (outcome, reason) = sync_history::classify_exit(status.code(), &stderr, &line);
    let all = format!("{stdout}\n{stderr}");
    let message = if all.contains("Invalid API key") || all.contains("UNAUTHORIZED") {
        "API Key 无效，请重新配置".into()
    } else {
        format!(
            "同步失败: {}",
            if line.is_empty() {
                format!("Exit code {}", status.code().unwrap_or(-1))
            } else {
                line
            }
        )
    };
    CliRun {
        cli_fallback,
        ..CliRun::failed(message, outcome, Some(reason))
    }
}

/// Drain stdout and stderr at the same time. Reading stdout to EOF before
/// touching stderr deadlocks as soon as the CLI writes more warnings than the
/// stderr pipe holds (small on Windows): the CLI blocks on that write, never
/// closes stdout, and the run ends as a 120-second "同步超时".
async fn read_both<O, E>(stdout: Option<O>, stderr: Option<E>) -> (String, String)
where
    O: tokio::io::AsyncRead + Unpin,
    E: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let out = async move {
        let mut text = String::new();
        if let Some(mut pipe) = stdout {
            let _ = pipe.read_to_string(&mut text).await;
        }
        text
    };
    let err = async move {
        let mut text = String::new();
        if let Some(mut pipe) = stderr {
            let _ = pipe.read_to_string(&mut text).await;
        }
        text
    };
    tokio::join!(out, err)
}

/// Pick the most informative line from CLI output. stderr carries non-fatal
/// parser warnings first and the fatal error near the end — but a Node crash
/// appends stack frames plus a "Node.js v22.x" footer AFTER the message, so
/// "last line" alone surfaces the useless footer. Scan from the bottom,
/// skipping crash-dump noise, and prefer an actual error line.
pub(crate) fn extract_error_line(msg: &str) -> String {
    let is_noise = |l: &str| {
        l.is_empty()
            || l.starts_with("Node.js v")
            || l.starts_with("at ") // stack frames
            || l.starts_with("code:")
            || l.starts_with('}')
            || l.starts_with('[')
            || l.starts_with("cause:")
            || l.starts_with("errno")
            || l.starts_with("syscall")
    };
    let lines: Vec<&str> = msg.lines().map(str::trim).collect();
    // 1) Bottom-up: a line that clearly states an error.
    if let Some(l) = lines.iter().rev().find(|l| {
        !is_noise(l)
            && (l.contains("Error") || l.contains("error") || l.contains('✗') || l.contains("失败"))
    }) {
        let mut s = l.to_string();
        s.truncate(s.char_indices().map(|(i, _)| i).nth(120).unwrap_or(s.len()));
        return s;
    }
    // 2) Bottom-up: last non-noise line.
    let mut s = lines
        .iter()
        .rev()
        .find(|l| !is_noise(l))
        .copied()
        .unwrap_or("")
        .to_string();
    s.truncate(s.char_indices().map(|(i, _)| i).nth(120).unwrap_or(s.len()));
    s
}

/// Full CLI output → the app log directory's `sync.log` (simple 512 KB cap)
/// so failures in the field are diagnosable.
fn write_sync_log(app: &AppHandle, status: &std::process::ExitStatus, stdout: &str, stderr: &str) {
    let entry = format!(
        "==== {} exit={:?} ====\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}\n\n",
        now_secs(),
        status.code()
    );
    append_sync_log(app, &entry);
    log::debug!("sync exit={status:?}");
}

/// A sync the app ended at `SYNC_TIMEOUT` leaves one fixed line, so the log
/// no longer skips exactly the runs that hung. No CLI output is written: the
/// run was cut off mid-way and its text is not kept. The version is the
/// newest complete one in the launcher's cache, which is what the launcher
/// runs unless that run was still downloading a newer `latest` or `latest`
/// moved back to an older cached version.
fn write_timeout_log(app: &AppHandle) {
    let version = crate::services::diagnostics::cached_cli_version(
        &app.state::<AppCtx>().config.config_dir.join("cli-cache"),
    );
    append_sync_log(app, &timeout_entry(now_secs(), version.as_deref()));
}

fn timeout_entry(now: u64, version: Option<&str>) -> String {
    format!(
        "==== {now} timeout={}s newest-cached-cli={} ====\n\n",
        SYNC_TIMEOUT.as_secs(),
        version.unwrap_or("none")
    )
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `%LOCALAPPDATA%\<identifier>\logs\sync.log` (Tauri 2's app log
/// directory), rotated to `sync.log.1` past 512 KB.
fn append_sync_log(app: &AppHandle, entry: &str) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("sync.log");
    if std::fs::metadata(&file).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&file, dir.join("sync.log.1"));
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(entry.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{read_both, run_with_follow_up, timeout_entry};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::{Mutex, Notify};

    async fn wait_true(flag: &AtomicBool) {
        let start = std::time::Instant::now();
        while !flag.load(Ordering::SeqCst) {
            assert!(
                start.elapsed() < std::time::Duration::from_secs(2),
                "timed out waiting for queued follow-up flag"
            );
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn overlapping_sync_request_is_not_dropped() {
        let running = Arc::new(Mutex::new(()));
        let pending = Arc::new(AtomicBool::new(false));
        let runs = Arc::new(AtomicUsize::new(0));
        let first_started = Arc::new(Notify::new());
        let release_first = Arc::new(Notify::new());

        let first = tokio::spawn({
            let running = running.clone();
            let pending = pending.clone();
            let runs = runs.clone();
            let first_started = first_started.clone();
            let release_first = release_first.clone();
            async move {
                run_with_follow_up(running.as_ref(), pending.as_ref(), || {
                    let runs = runs.clone();
                    let first_started = first_started.clone();
                    let release_first = release_first.clone();
                    async move {
                        let n = runs.fetch_add(1, Ordering::SeqCst);
                        if n == 0 {
                            first_started.notify_waiters();
                            release_first.notified().await;
                        }
                    }
                })
                .await;
            }
        });

        first_started.notified().await;

        let second = tokio::spawn({
            let running = running.clone();
            let pending = pending.clone();
            let runs = runs.clone();
            async move {
                run_with_follow_up(running.as_ref(), pending.as_ref(), || {
                    let runs = runs.clone();
                    async move {
                        runs.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .await;
            }
        });

        wait_true(pending.as_ref()).await;
        release_first.notify_waiters();
        first.await.unwrap();
        second.await.unwrap();

        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "in-flight sync plus the queued post-config request"
        );
    }

    #[tokio::test]
    async fn overlapping_requests_coalesce_to_one_follow_up() {
        let running = Arc::new(Mutex::new(()));
        let pending = Arc::new(AtomicBool::new(false));
        let runs = Arc::new(AtomicUsize::new(0));
        let first_started = Arc::new(Notify::new());
        let release_first = Arc::new(Notify::new());

        let first = tokio::spawn({
            let running = running.clone();
            let pending = pending.clone();
            let runs = runs.clone();
            let first_started = first_started.clone();
            let release_first = release_first.clone();
            async move {
                run_with_follow_up(running.as_ref(), pending.as_ref(), || {
                    let runs = runs.clone();
                    let first_started = first_started.clone();
                    let release_first = release_first.clone();
                    async move {
                        let n = runs.fetch_add(1, Ordering::SeqCst);
                        if n == 0 {
                            first_started.notify_waiters();
                            release_first.notified().await;
                        }
                    }
                })
                .await;
            }
        });

        first_started.notified().await;

        for _ in 0..3 {
            let running = running.clone();
            let pending = pending.clone();
            tokio::spawn(async move {
                run_with_follow_up(running.as_ref(), pending.as_ref(), || async {}).await;
            });
        }

        wait_true(pending.as_ref()).await;
        release_first.notify_waiters();
        first.await.unwrap();

        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn single_sync_request_runs_once() {
        let running = Mutex::new(());
        let pending = AtomicBool::new(false);
        let runs = AtomicUsize::new(0);
        run_with_follow_up(&running, &pending, || {
            runs.fetch_add(1, Ordering::SeqCst);
            async {}
        })
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(!pending.load(Ordering::SeqCst));
    }

    /// #131 follow-up: a timed-out sync used to leave nothing in sync.log.
    /// Its line holds only fixed fields: time, the timeout, a version.
    #[test]
    fn a_timeout_leaves_one_fixed_line() {
        assert_eq!(
            timeout_entry(1_794_205_920, Some("0.14.11")),
            "==== 1794205920 timeout=120s newest-cached-cli=0.14.11 ====\n\n"
        );
        assert_eq!(
            timeout_entry(1, None),
            "==== 1 timeout=120s newest-cached-cli=none ====\n\n"
        );
    }

    /// A CLI that writes far more to stderr than a pipe holds, and only then
    /// to stdout, must still be read to the end (the old code read stdout to
    /// EOF first and deadlocked until the 120-second timeout).
    #[tokio::test]
    async fn heavy_stderr_output_does_not_block_reading_stdout() {
        use tokio::io::AsyncWriteExt;
        let (mut stdout_writer, stdout_reader) = tokio::io::duplex(64);
        let (mut stderr_writer, stderr_reader) = tokio::io::duplex(64);
        let warnings = "warning: skipped one file\n".repeat(20_000);
        let expected = warnings.len();
        let writer = tokio::spawn(async move {
            stderr_writer.write_all(warnings.as_bytes()).await.unwrap();
            drop(stderr_writer);
            stdout_writer.write_all(b"Synced 12 buckets").await.unwrap();
        });
        let (stdout, stderr) = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            read_both(Some(stdout_reader), Some(stderr_reader)),
        )
        .await
        .expect("both pipes must drain without waiting for each other");
        writer.await.unwrap();
        assert_eq!(stdout, "Synced 12 buckets");
        assert_eq!(stderr.len(), expected);
    }
}
