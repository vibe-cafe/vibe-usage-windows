//! CLI sync subprocess — port of Services/SyncEngine.swift.
//!
//! Runs `node <resources>/cli/bin/vibe-usage.js sync` (the vendored
//! @vibe-cafe/vibe-usage CLI) with a 120s timeout and no console window.

use crate::process_utils;
use crate::state::{AppCtx, SyncState, SyncStatus};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use vibe_core::runtime::{self, Runtime, RuntimeKind};

pub const SYNC_TIMEOUT: Duration = Duration::from_secs(120);

fn resource_path(app: &AppHandle, rel: &str) -> Option<PathBuf> {
    app.path().resolve(rel, tauri::path::BaseDirectory::Resource).ok()
}

/// The vendored CLI entry script.
pub fn cli_entry(app: &AppHandle) -> Option<PathBuf> {
    let p = resource_path(app, "cli/bin/vibe-usage.js")?;
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
    let out = cmd.output().ok()?;
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

/// Run one sync. Concurrent calls return immediately (同步已在进行中).
pub async fn run_sync(app: AppHandle) {
    let ctx = app.state::<AppCtx>();

    let Ok(_guard) = ctx.sync_running.try_lock() else {
        log::info!("sync already running");
        return;
    };

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
    let Some(cli) = cli_entry(app) else {
        return Err("同步失败: 未找到内置 CLI 资源".into());
    };
    let Some(rt) = detect_runtime(app) else {
        return Err("未检测到可用的 Node.js 运行时，请安装 Node.js 22+".into());
    };
    log::info!("sync via {:?} {}", rt.kind, rt.path.display());

    let mut cmd = tokio::process::Command::new(&rt.path);
    cmd.arg(&cli)
        .arg("sync")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Ensure the runtime's directory is in PATH (mirrors SyncEngine).
    if let Some(dir) = rt.path.parent() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}{sep}{path}", dir.display()));
    }
    if crate::state::IS_DEV {
        cmd.env("VIBE_USAGE_DEV", "1");
    }
    process_utils::hide_tokio_command_window(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| format!("同步失败: {e}"))?;
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
    log::debug!("sync exit={status:?} stdout={} stderr={}", &stdout, &stderr);

    if status.success() {
        // "Synced …" / "No new usage data" both count as success.
        Ok(if stdout.is_empty() { "同步完成".into() } else { stdout })
    } else {
        let all = format!("{stdout}\n{stderr}");
        if all.contains("Invalid API key") || all.contains("UNAUTHORIZED") {
            Err("API Key 无效，请重新配置".into())
        } else {
            let msg = if stderr.is_empty() { stdout } else { stderr };
            Err(format!(
                "同步失败: {}",
                if msg.is_empty() {
                    format!("Exit code {}", status.code().unwrap_or(-1))
                } else {
                    msg.lines().next().unwrap_or(&msg).to_string()
                }
            ))
        }
    }
}
