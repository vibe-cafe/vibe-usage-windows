//! Self-update — Windows counterpart of Sparkle (custom flow, same as ATM):
//! fetch latest.json from GitHub Releases, compare versions, download the
//! NSIS installer, verify SHA-256, launch it and exit.

use crate::state::{AppCtx, UpdateInfo};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

pub const UPDATE_MANIFEST_URL: &str =
    "https://github.com/haoruilee/vibe-usage-app-windows/releases/latest/download/latest.json";

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    url: String,
    #[serde(default)]
    sha256: Option<String>,
}

/// Check for a newer version; stores + emits `update-available` when found.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let http = {
        let ctx = app.state::<AppCtx>();
        ctx.http.clone()
    };
    let res = http
        .get(UPDATE_MANIFEST_URL)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("网络错误：{e}"))?;
    if !res.status().is_success() {
        return Err(format!("HTTP 错误 {}", res.status().as_u16()));
    }
    let manifest: Manifest = res.json().await.map_err(|e| format!("清单解析失败：{e}"))?;

    let current = app.package_info().version.to_string();
    if !vibe_core::version::is_newer(&manifest.version, &current) {
        return Ok(None);
    }

    let info = UpdateInfo {
        version: manifest.version,
        notes: manifest.notes,
        url: manifest.url,
        sha256: manifest.sha256,
    };
    {
        let ctx = app.state::<AppCtx>();
        *ctx.update_info.lock().unwrap() = Some(info.clone());
    }
    let _ = app.emit("update-available", &info);
    Ok(Some(info))
}

/// Download + verify + launch the installer, then quit so it can replace us.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let info = {
        let ctx = app.state::<AppCtx>();
        let info = ctx.update_info.lock().unwrap().clone();
        info
    }
    .ok_or("没有可安装的更新")?;

    if !cfg!(windows) {
        return Err("自动更新仅支持 Windows".into());
    }

    let http = {
        let ctx = app.state::<AppCtx>();
        ctx.http.clone()
    };
    let bytes = http
        .get(&info.url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("下载失败：{e}"))?
        .bytes()
        .await
        .map_err(|e| format!("下载失败：{e}"))?;

    if let Some(expected) = &info.sha256 {
        let actual = hex::encode(Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            return Err("安装包校验失败 (SHA-256 不匹配)".into());
        }
    }

    let installer = std::env::temp_dir().join(format!("VibeUsage-{}-Setup.exe", info.version));
    std::fs::write(&installer, &bytes).map_err(|e| format!("写入安装包失败：{e}"))?;

    crate::process_utils::shell_open(&installer.to_string_lossy())
        .map_err(|e| format!("启动安装器失败：{e}"))?;
    app.exit(0);
    Ok(())
}
