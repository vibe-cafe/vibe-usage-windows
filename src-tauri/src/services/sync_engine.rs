//! CLI sync subprocess — port of Services/SyncEngine.swift.
//!
//! Runs `node <resources>/cli-bootstrap.mjs sync`, which resolves
//! `@vibe-cafe/vibe-usage@latest` from the registry, caches it, and runs it with
//! this app's bundled Node — 120s timeout, no console window. The CLI is *not*
//! bundled: a pinned copy rots silently and freezes users out of every CLI fix,
//! which is the policy the macOS app already follows (`@latest` only).
use crate::process_utils;
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
    let stderr = String::from_utf8_lossy(&output.stderr);
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

/// Run one sync. Concurrent calls queue a follow-up instead of being dropped.
pub async fn run_sync(app: AppHandle) {
    let work_app = app.clone();
    let ctx = app.state::<AppCtx>();
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

    let result = run_cli_sync(&app).await;

    match result {
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

async fn run_cli_sync(app: &AppHandle) -> Result<String, String> {
    let mut cmd = cli_command(app, &["sync"]).map_err(|e| format!("同步失败: {e}"))?;

    let (mut child, _process_guard) = crate::process_lifecycle::spawn(&mut cmd)
        .map_err(|e| format!("同步失败: {e}"))?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    // Read pipes concurrently with waiting so the child never blocks on a
    // full pipe buffer.
    let io_task = async {
        use tokio::io::AsyncReadExt;
        let mut out = String::new();
        let mut err = String::new();
        if let Some(mut p) = stdout_pipe {
            let _ = p.read_to_string(&mut out).await;
        }
        if let Some(mut p) = stderr_pipe {
            let _ = p.read_to_string(&mut err).await;
        }
        (out, err)
    };

    let combined = tokio::select! {
        res = async {
            let (io, status) = tokio::join!(io_task, child.wait());
            (io, status)
        } => res,
        _ = tokio::time::sleep(SYNC_TIMEOUT) => {
            process_utils::kill_child_tree(&mut child);
            return Err("同步超时".into());
        }
    };

    let ((stdout, stderr), status) = combined;
    let status = status.map_err(|e| format!("同步失败: {e}"))?;
    let stdout = stdout.trim().to_string();
    let stderr = stderr.trim().to_string();
    write_sync_log(app, &status, &stdout, &stderr);

    if status.success() {
        // "Synced …" / "No new usage data" both count as success.
        Ok(if stdout.is_empty() { "同步完成".into() } else { stdout })
    } else {
        let all = format!("{stdout}\n{stderr}");
        if all.contains("Invalid API key") || all.contains("UNAUTHORIZED") {
            Err("API Key 无效，请重新配置".into())
        } else {
            let msg = if stderr.is_empty() { &stdout } else { &stderr };
            let line = extract_error_line(msg);
            Err(format!(
                "同步失败: {}",
                if line.is_empty() {
                    format!("Exit code {}", status.code().unwrap_or(-1))
                } else {
                    line
                }
            ))
        }
    }
}

/// Pick the most informative line from CLI output. stderr carries non-fatal
/// parser warnings first and the fatal error near the end — but a Node crash
/// appends stack frames plus a "Node.js v22.x" footer AFTER the message, so
/// "last line" alone surfaces the useless footer. Scan from the bottom,
/// skipping crash-dump noise, and prefer an actual error line.
fn extract_error_line(msg: &str) -> String {
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

/// Full CLI output → %APPDATA%/<identifier>/logs/sync.log (simple 512 KB cap)
/// so failures in the field are diagnosable.
fn write_sync_log(app: &AppHandle, status: &std::process::ExitStatus, stdout: &str, stderr: &str) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("sync.log");
    if std::fs::metadata(&file).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&file, dir.join("sync.log.1"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = format!(
        "==== {now} exit={:?} ====\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}\n\n",
        status.code()
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(entry.as_bytes());
    }
    log::debug!("sync exit={status:?}; log → {}", file.display());
}

#[cfg(test)]
mod tests {
    use super::run_with_follow_up;
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
}
