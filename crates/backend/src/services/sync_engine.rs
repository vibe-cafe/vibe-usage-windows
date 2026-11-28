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
use crate::state::{SyncState, SyncStatus};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use crate::Backend;
use vibe_core::runtime::{self, Runtime, RuntimeKind};

pub const SYNC_TIMEOUT: Duration = Duration::from_secs(120);


/// The CLI launcher script. It resolves and runs the published CLI, so nothing
/// here depends on a particular CLI version.
pub fn cli_entry(app: &Backend) -> Option<PathBuf> {
    let p = app.options().resources.join("cli-bootstrap.mjs");
    p.is_file().then_some(p)
}

/// The bundled node.exe (Windows release bundles only; absent in dev).
pub fn bundled_node(app: &Backend) -> Option<PathBuf> {
    let p = app.options().resources.join("node/node.exe");
    p.is_file().then_some(p)
}

fn probe_node_version(path: &std::path::Path) -> Option<(u32, u32, u32)> {
    let mut cmd = std::process::Command::new(path);
    cmd.arg("-v").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
    process_utils::hide_command_window(&mut cmd);
    let out = crate::process_lifecycle::output_sync(&mut cmd).ok()?;
    runtime::parse_node_version(&String::from_utf8_lossy(&out.stdout))
}

pub fn detect_runtime(app: &Backend) -> Option<Runtime> {
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
pub fn node_for_statusline(app: &Backend) -> PathBuf {
    match detect_runtime(app) {
        Some(rt) if rt.kind != RuntimeKind::Bun => rt.path,
        _ => PathBuf::from("node"),
    }
}

fn cli_command(app: &Backend, args: &[&str]) -> Result<tokio::process::Command, String> {
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
        app.ctx().config.config_dir.clone(),
    );
    cmd.env(
        "VIBE_USAGE_STATE_DIR",
        app.ctx().config.config_dir.clone(),
    );
    cmd.env("VIBE_USAGE_SURFACE", "windows-app");
    cmd.env(
        "VIBE_USAGE_SURFACE_VERSION",
        app.options().version.clone(),
    );
    if crate::state::IS_DEV {
        cmd.env("VIBE_USAGE_DEV", "1");
    }
    // The system proxy (manual list, per-protocol entries, PAC evaluated by
    // WinHTTP), read now for the hosts the CLI reaches (`system_proxy`).
    let lookup = |name: &str| std::env::var(name).ok();
    let routes = if crate::system_proxy::has_own_proxy(&lookup) {
        Vec::new()
    } else {
        let npmrc = dirs::home_dir().and_then(|home| std::fs::read_to_string(home.join(".npmrc")).ok());
        let api_url = app.ctx().config.load_api_url();
        crate::system_proxy::live_routes(&crate::system_proxy::cli_urls(npmrc.as_deref(), Some(&api_url)))
    };
    for (name, value) in crate::system_proxy::proxy_variables(&routes, &lookup, true, true) {
        cmd.env(name, value);
    }
    // The egress-check settings, for the reads the CLI makes itself
    // (Antigravity): `egress_check::Settings::cli_environment`.
    let egress = app.ctx().settings.lock().unwrap().egress();
    for (name, value) in egress.cli_environment() {
        cmd.env(name, value);
    }
    process_utils::hide_tokio_command_window(&mut cmd);
    Ok(cmd)
}

/// Build a command for the typed quota bridge. Arguments are owned by the
/// caller but copied into Command before this function returns.
pub(super) fn quota_command(
    app: &Backend,
    args: &[String],
) -> Result<tokio::process::Command, String> {
    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    cli_command(app, &borrowed)
}

/// The Cursor sign-in commands (`services::cursor_login`): same runtime,
/// directories, surface and proxy as a sync. What they print is the sign-in
/// itself, so callers never log it.
pub(super) fn cursor_login_command(app: &Backend, args: &[&str]) -> Result<tokio::process::Command, String> {
    cli_command(app, args)
}

/// `doctor --json` for 「复制诊断信息」, with the same runtime, directories,
/// surface and proxy as a sync (`services::cli_doctor`).
pub(super) fn doctor_command(app: &Backend) -> Result<tokio::process::Command, String> {
    cli_command(app, &["doctor", "--json"])
}

/// Run a short config command against the same bundled CLI and config directory
/// used by sync.
pub async fn run_config_command(app: &Backend, args: &[&str]) -> Result<String, String> {
    app.ctx().wait_launch_network().await;
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

fn set_state(app: &Backend, update: impl FnOnce(&mut SyncState)) {
    let ctx = app.ctx();
    let snapshot = {
        let mut state = ctx.sync_state.lock().unwrap();
        update(&mut state);
        state.clone()
    };
    let _ = app.emit("sync-state", &snapshot);
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
pub async fn run_sync(app: Backend, trigger: SyncTrigger) {
    let work_app = app.clone();
    let ctx = app.ctx();
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

async fn run_sync_once(app: Backend) {
    let ctx = app.ctx();
    let generation = *ctx.account_generation.lock().unwrap();

    if !ctx.config.is_configured() {
        return;
    }

    set_state(&app, |s| {
        s.status = SyncStatus::Syncing;
        s.message = None;
    });
    // A login launch: no sync, manual ones included, before the user's proxy
    // is up (`AppCtx::wait_launch_network`). A reset or relink meanwhile
    // drops this run.
    let account = ctx.config.load().and_then(|config| config.api_key);
    ctx.wait_launch_network().await;
    if ctx.config.load().and_then(|config| config.api_key) != account {
        set_state(&app, |s| {
            s.status = SyncStatus::Idle;
            s.message = None;
        });
        return;
    }

    let trigger = if ctx.sync_requested_manually.load(Ordering::SeqCst) {
        SyncTrigger::Manual
    } else {
        SyncTrigger::Scheduled
    };
    let started_at = app.0.network.now_ms();
    let started = std::time::Instant::now();
    let run = app.0.network.sync(&app).await;
    let current = ctx.account_generation.lock().unwrap();
    if *current != generation { return; }
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
            let last = app.0.network.now_ms();
            if ctx.update_settings(|s| s.last_successful_sync_at = Some(last)).is_err() {
                log::warn!("failed to save last successful sync");
            }
            set_state(&app, |s| {
                s.status = SyncStatus::Success;
                s.message = Some(message);
                s.last_sync_at = Some(last);
                s.last_success = Some(tokio::time::Instant::now());
            });
            app.refresh_usage(true);
            // Reset to idle after 3s (mirrors AppState.triggerSync).
            let app2 = app.clone();
            app.spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let ctx = app2.ctx();
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
pub(crate) struct CliRun {
    pub(crate) message: Result<String, String>,
    pub(crate) outcome: SyncOutcome,
    pub(crate) reason: Option<SyncReason>,
    /// The launcher ran a cached CLI because `latest` was out of reach.
    pub(crate) cli_fallback: Option<LauncherStage>,
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

/// The CLI's notice when the Cursor sign-in it was handed has ended
/// (`LOGIN_EXPIRED_NOTICE`, CLI `src/parsers/cursor.js`; its warning code
/// `cursor_login_expired` matches the same words).
const CURSOR_LOGIN_ENDED: &str = "Cursor 登录已失效";
/// Every Cursor warning or error the CLI prints during a sync starts so
/// (`  cursor: …`, `src/sync.js`).
const CURSOR_NOTICE: &str = "cursor: ";

pub(crate) async fn run_cli_sync(app: &Backend) -> CliRun {
    // The app's own Cursor sign-in goes on stdin, renewed first when due --
    // before the command reads the proxy, as a renewal may take a minute.
    let cursor = crate::services::cursor_login::for_cli(app).await;
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

    // Checked again right before the start: reading the proxy, PAC included,
    // can take seconds, and a sign-out meanwhile must hold.
    let cursor = crate::services::cursor_login::recheck(cursor);
    if cursor.is_some() {
        cmd.arg("--cursor-login-stdin").stdin(Stdio::piped());
    }
    let (mut child, _process_guard) = match crate::process_lifecycle::spawn(&mut cmd) {
        Ok(spawned) => spawned,
        Err(e) => {
            log::warn!("sync: could not start the runtime: {e}");
            return CliRun::failed(
                failure_message(&SyncReason::SpawnFailed, "", ""),
                SyncOutcome::CliStartupFailed,
                Some(SyncReason::SpawnFailed),
            );
        }
    };
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdin_write = write_stdin(child.stdin.take(), cursor.as_ref().map(|handover| handover.input.clone()));

    // Read both pipes concurrently with waiting so the child never blocks on
    // a full pipe buffer.
    let io_task = read_both(stdout_pipe, stderr_pipe);

    let combined = tokio::select! {
        res = async {
            let (_, io, status) = tokio::join!(stdin_write, io_task, child.wait());
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
            log::warn!("sync: waiting for the CLI failed: {e}");
            return CliRun::failed(
                failure_message(&SyncReason::WaitFailed, "", ""),
                SyncOutcome::Failed,
                Some(SyncReason::WaitFailed),
            );
        }
    };
    let stdout = stdout.trim().to_string();
    let stderr = stderr.trim().to_string();
    write_sync_log(app, &status, &stdout, &stderr);
    // Cursor rejected the sign-in (or it lapsed): renew it once in the
    // background; Cursor refusing that ends it. The next sync reads again and
    // the CLI fills the skipped weeks.
    if let Some(handover) = cursor.filter(|handover| handover.is_live()) {
        if stdout.contains(CURSOR_LOGIN_ENDED) || stderr.contains(CURSOR_LOGIN_ENDED) {
            let worker = app.clone();
            app.spawn(async move {
                crate::services::cursor_login::after_rejection(&worker, &handover).await;
            });
        } else if status.success() && !stderr.contains(CURSOR_NOTICE) {
            // Cursor answered without a word of trouble: the sign-in works.
            crate::services::cursor_login::read_ok(app, &handover);
        }
    }
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
    let message = failure_message(&reason, &line, &all);
    CliRun {
        cli_fallback,
        ..CliRun::failed(message, outcome, Some(reason))
    }
}

/// Markers of a request that never reached the sync server (Node's error
/// codes and messages), read from the run's final error line only: an earlier
/// tool warning (e.g. a skipped Cursor export) may carry one too. Same codes
/// as the CLI's own `network_failed` category (`src/sync-history.js`).
const NETWORK_MARKERS: [&str; 13] = [
    "fetch failed",
    "ECONNREFUSED",
    "ECONNRESET",
    "ECONNABORTED",
    "ENOTFOUND",
    "EAI_AGAIN",
    "ETIMEDOUT",
    "EHOSTUNREACH",
    "ENETUNREACH",
    "ENETDOWN",
    "EPIPE",
    "socket hang up",
    "secure TLS connection",
];

/// The CLI's key hint (`API Key 无效，请运行 … 重新配置。`), the older
/// English markers, and a bare 401. A 403 alone is not the key: the CLI keeps
/// it as an HTTP error, and a proxy or access rule can send it too.
const KEY_MARKERS: [&str; 4] = ["API Key 无效", "Invalid API key", "UNAUTHORIZED", "HTTP 401"];

const KEY_REJECTED: &str = "API Key 无效，请重新配置";

/// The status bar's line for a failed run: our own short sentence chosen from
/// the run's typed category and final error line, never the raw output of the
/// CLI or the launcher (English, sometimes with local paths or the server's
/// response body, and too long for the tray tooltip). The CLI's own Chinese
/// hints keep their meaning in our words; the full output stays in `sync.log`.
/// The launcher sentences and the crash case are macOS's
/// (`SyncEngine.launcherFailureText`, `friendlyFailureMessage`); macOS still
/// shows the raw line for the rest. `final_line` is the run's error line
/// (`extract_error_line`), `output` all of stdout and stderr.
fn failure_message(reason: &SyncReason, final_line: &str, output: &str) -> String {
    // `部分完成（已上传 …）: <reason>`: the other tools' data did arrive.
    let partial = final_line.contains("部分完成");
    let head = if partial { "同步部分完成" } else { "同步失败" };
    // The CLI held Codex back (its closing line, `CODEX_ULTRAFAST_HOLD_FAILURE`).
    if final_line.contains("Codex 本次未上传") {
        let head = if partial { head } else { "同步未完成" };
        return format!("{head}: Codex 本次未上传，下次同步自动补传");
    }
    if KEY_MARKERS.iter().any(|marker| final_line.contains(marker)) {
        return if partial {
            format!("{head}: {KEY_REJECTED}")
        } else {
            KEY_REJECTED.into()
        };
    }
    let detail = match reason {
        SyncReason::Launcher { stage } => match stage {
            LauncherStage::Network => "连不上 npm 源，CLI 没有下载成功".to_string(),
            LauncherStage::Registry => "npm 源返回错误，CLI 没有下载成功".to_string(),
            LauncherStage::Download => "CLI 下载或校验失败".to_string(),
            LauncherStage::Install => "CLI 安装失败".to_string(),
            LauncherStage::Other => "CLI 启动失败".to_string(),
        },
        SyncReason::ServerStatus { status } => format!("服务器返回错误 {status}"),
        // The history counts 401 / 403 as a rejected key; a 401 or the CLI's
        // key hint returned above, so what is left is a bare 403.
        SyncReason::KeyRejected => "服务器返回错误 403".to_string(),
        SyncReason::RuntimeUnavailable | SyncReason::SpawnFailed => "同步工具启动失败".to_string(),
        SyncReason::WaitFailed => "同步工具异常退出".to_string(),
        SyncReason::CliExit { exit_code } => cli_exit_detail(*exit_code, final_line, output),
    };
    format!("{head}: {detail}")
}

/// A CLI run that ended without a status number: the CLI's own errors, named
/// by the fixed start of their messages (`src/api.js`, `src/sync.js`).
fn cli_exit_detail(exit_code: i32, final_line: &str, output: &str) -> String {
    if output.contains("RangeError: Invalid string length") || output.contains("node:internal/readline")
    {
        "CLI 读取本机历史记录时崩溃".into()
    } else if final_line.contains("暂时无法读取上传设置") {
        "暂时读不到上传设置，未上传数据，请稍后重试".into()
    } else if final_line.contains("Request timed out") {
        "连接同步服务器超时".into()
    } else if final_line.contains("Invalid JSON response") {
        "服务器返回了无法识别的数据".into()
    } else if final_line.to_ascii_lowercase().contains("certificate") || final_line.contains("CERT_") {
        "同步服务器的证书校验失败".into()
    } else if NETWORK_MARKERS.iter().any(|marker| final_line.contains(marker)) {
        "连不上同步服务器".into()
    } else {
        format!("退出码 {exit_code}")
    }
}

/// Hand `input` to the child's stdin and close it (`--cursor-login-stdin`
/// reads to the end). Nothing to write: stdin was never opened.
async fn write_stdin(stdin: Option<tokio::process::ChildStdin>, input: Option<Vec<u8>>) {
    use tokio::io::AsyncWriteExt;
    if let (Some(mut stdin), Some(input)) = (stdin, input) {
        // A CLI that exits without reading says so in its own output.
        let _ = stdin.write_all(&input).await;
        let _ = stdin.shutdown().await;
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
    // 0) The CLI's own failure line (`✗ …`, `src/output.js`): it prints one
    //    only right before exiting, and an HTTP error appends the server's
    //    response body after it, so a multi-line error page would otherwise
    //    win step 1 with whatever HTML line says "Error".
    if let Some(l) = lines.iter().rev().find(|l| l.contains('✗')) {
        let mut s = l.to_string();
        s.truncate(s.char_indices().map(|(i, _)| i).nth(120).unwrap_or(s.len()));
        return s;
    }
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
fn write_sync_log(app: &Backend, status: &std::process::ExitStatus, stdout: &str, stderr: &str) {
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
fn write_timeout_log(app: &Backend) {
    let version = crate::services::diagnostics::cached_cli_version(
        &app.ctx().config.config_dir.join("cli-cache"),
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
fn append_sync_log(app: &Backend, entry: &str) {
    let dir = app.options().local_data.join("logs");
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
    use super::{extract_error_line, failure_message, read_both, run_with_follow_up, timeout_entry};
    use crate::services::sync_history::{classify_exit, LauncherStage, SyncReason};
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

    /// The status bar line for a run that exited 1 with this stderr, through
    /// the same classification the sync uses.
    fn bar(stderr: &str) -> String {
        let line = extract_error_line(stderr);
        let (_, reason) = classify_exit(Some(1), stderr, &line);
        failure_message(&reason, &line, stderr)
    }

    #[test]
    fn failed_sync_shows_a_short_chinese_sentence_never_the_raw_line() {
        assert_eq!(bar("✗ 同步失败: fetch failed"), "同步失败: 连不上同步服务器");
        assert_eq!(
            bar("✗ 同步失败: getaddrinfo ENOTFOUND vibefriends.com"),
            "同步失败: 连不上同步服务器"
        );
        assert_eq!(
            bar("✗ 同步失败: HTTP 502: <html>Bad gateway</html>"),
            "同步失败: 服务器返回错误 502"
        );
        assert_eq!(
            bar("✗ 部分完成（已上传 12 buckets）: HTTP 503: Service Unavailable"),
            "同步部分完成: 服务器返回错误 503"
        );
        assert_eq!(
            bar("✗ API Key 无效，请运行 `npx @vibe-cafe/vibe-usage init` 重新配置。"),
            "API Key 无效，请重新配置"
        );
        assert_eq!(bar("Error: UNAUTHORIZED"), "API Key 无效，请重新配置");
        // A bare 403 (a proxy or access rule) is not a rejected key.
        assert_eq!(
            bar("✗ 同步失败: HTTP 403: <html>Forbidden</html>"),
            "同步失败: 服务器返回错误 403"
        );
        assert_eq!(
            bar("✗ 同步失败: Request timed out (60s)"),
            "同步失败: 连接同步服务器超时"
        );
        assert_eq!(
            bar("✗ 同步失败: Invalid JSON response: <html>captive portal</html>"),
            "同步失败: 服务器返回了无法识别的数据"
        );
        assert_eq!(
            bar("✗ 同步失败: self-signed certificate in certificate chain"),
            "同步失败: 同步服务器的证书校验失败"
        );
        assert_eq!(
            bar("✗ 同步失败: Client network socket disconnected before secure TLS connection was established"),
            "同步失败: 连不上同步服务器"
        );
        // An error page appended after the CLI's line: the ✗ line still decides.
        assert_eq!(
            bar("✗ 部分完成（已上传 12 buckets）: HTTP 502: <!DOCTYPE html>\n<html>\n<head><title>vibefriends.com | 502: Bad gateway</title></head>\n<body><span>Error code 502</span>\n</body></html>"),
            "同步部分完成: 服务器返回错误 502"
        );
        assert_eq!(
            bar("✗ 同步失败: HTTP 403: <html>\n<head><title>Login Error</title></head>\n</html>"),
            "同步失败: 服务器返回错误 403"
        );
        let unknown = bar("✗ 同步失败: Cannot read properties of undefined (reading 'x') at C:\\Users\\me\\a.js");
        assert_eq!(unknown, "同步失败: 退出码 1");
    }

    #[test]
    fn the_clis_own_chinese_hints_keep_their_meaning() {
        // Other tools did upload: the line says so, and Codex waits.
        assert_eq!(
            bar("✗ 部分完成（已上传 3 buckets）: Codex 本次未上传：服务端暂时没有答复这台设备的 Ultrafast 档位起点，其他工具已正常同步，下次同步自动补传。"),
            "同步部分完成: Codex 本次未上传，下次同步自动补传"
        );
        assert_eq!(
            bar("✗ 同步未完成: Codex 本次未上传：服务端暂时没有答复这台设备的 Ultrafast 档位起点，其他工具已正常同步，下次同步自动补传。"),
            "同步未完成: Codex 本次未上传，下次同步自动补传"
        );
        assert_eq!(
            bar("✗ 部分完成（已上传 12 buckets）: API Key 无效，请运行 `npx @vibe-cafe/vibe-usage init` 重新配置。"),
            "同步部分完成: API Key 无效，请重新配置"
        );
        assert_eq!(
            bar("✗ 暂时无法读取上传设置，本次同步已安全取消（未上传数据）。请稍后重试。"),
            "同步失败: 暂时读不到上传设置，未上传数据，请稍后重试"
        );
        assert_eq!(
            bar("file:///x/parser.js:10\nRangeError: Invalid string length\n    at JSON.stringify (<anonymous>)\nNode.js v22.23.1"),
            "同步失败: CLI 读取本机历史记录时崩溃"
        );
    }

    #[test]
    fn launcher_and_process_failures_use_the_macos_sentences() {
        let launcher = |stage| failure_message(&SyncReason::Launcher { stage }, "", "");
        assert_eq!(launcher(LauncherStage::Network), "同步失败: 连不上 npm 源，CLI 没有下载成功");
        assert_eq!(launcher(LauncherStage::Registry), "同步失败: npm 源返回错误，CLI 没有下载成功");
        assert_eq!(launcher(LauncherStage::Download), "同步失败: CLI 下载或校验失败");
        assert_eq!(launcher(LauncherStage::Install), "同步失败: CLI 安装失败");
        assert_eq!(launcher(LauncherStage::Other), "同步失败: CLI 启动失败");
        assert_eq!(
            failure_message(&SyncReason::SpawnFailed, "", ""),
            "同步失败: 同步工具启动失败"
        );
        assert_eq!(
            failure_message(&SyncReason::WaitFailed, "", ""),
            "同步失败: 同步工具异常退出"
        );
    }
}
