//! Self-update — Windows counterpart of Sparkle (custom flow, same as ATM):
//! fetch latest.json from GitHub Releases, compare versions, download the
//! NSIS installer, verify SHA-256, launch it in update mode and exit.
//! See docs/WINDOWS_UPDATE.md.

use crate::state::{AppCtx, UpdateInfo};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const UPDATE_MANIFEST_URL: &str =
    "https://github.com/vibe-cafe/vibe-usage-windows/releases/latest/download/latest.json";

/// Switches for the Tauri NSIS template, the same ones tauri-plugin-updater
/// passes: `/P` passive (progress bar only, closes itself), `/UPDATE` overwrite
/// the installed copy without running the previous uninstaller, `/R` relaunch
/// the app when done. Approved under the Architecture Approval Gate (Hogan,
/// 2026-11-11).
pub const INSTALLER_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];

fn installer_command(installer: &Path) -> std::process::Command {
    crate::process_utils::executable_command(installer, &INSTALLER_ARGS)
}

/// One install at a time: a second click while the first download is still
/// running would otherwise start a second passive installer over the same files.
static INSTALLING: AtomicBool = AtomicBool::new(false);

struct InstallGuard;

impl InstallGuard {
    fn acquire() -> Option<Self> {
        INSTALLING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| InstallGuard)
    }

    /// The installer is running and the app is exiting: stay locked.
    fn keep(self) {
        std::mem::forget(self);
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::Release);
    }
}

pub const fn available() -> bool {
    !cfg!(feature = "external-test-diagnostics")
}

fn require_updates() -> Result<(), String> {
    if available() {
        Ok(())
    } else {
        Err("外测版不检查或安装正式版更新".into())
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    url: String,
    #[serde(default)]
    sha256: Option<String>,
}

/// Check for a newer version. Found: store it and emit `update-available`.
/// The manifest names no newer version: drop any update found earlier and
/// emit `update-cleared`, so a release that was pulled (the previous version
/// marked latest again, see docs/WINDOWS_UPDATE.md 「撤回」) stops being
/// offered and its download address is forgotten. A failed check changes
/// nothing.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    require_updates()?;
    let http = {
        let ctx = app.state::<AppCtx>();
        ctx.http.clone()
    };
    let info = fetch_update(
        &http,
        &app.package_info().version.to_string(),
        UPDATE_MANIFEST_URL,
    )
    .await?;
    let cleared = {
        let ctx = app.state::<AppCtx>();
        record_check(&ctx.update_info, info.as_ref())
    };
    if let Some(ref info) = info {
        let _ = app.emit("update-available", info);
    } else if cleared {
        let _ = app.emit("update-cleared", ());
    }
    Ok(info)
}

/// Store the result of a successful check. Returns whether an update found
/// earlier was dropped because the manifest no longer names a newer version.
fn record_check(slot: &Mutex<Option<UpdateInfo>>, found: Option<&UpdateInfo>) -> bool {
    let mut slot = slot.lock().unwrap();
    let had_update = slot.is_some();
    *slot = found.cloned();
    had_update && found.is_none()
}

async fn fetch_update(
    http: &reqwest::Client,
    current: &str,
    manifest_url: &str,
) -> Result<Option<UpdateInfo>, String> {
    require_updates()?;
    let res = http
        .get(manifest_url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("网络错误：{e}"))?;
    let status = res.status();
    if status.as_u16() == 404 {
        log::info!("update manifest not found; treating as no update");
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("HTTP 错误 {}", status.as_u16()));
    }
    let manifest: Manifest = res.json().await.map_err(|e| format!("清单解析失败：{e}"))?;

    if !vibe_core::version::is_newer(&manifest.version, current) {
        return Ok(None);
    }

    let info = UpdateInfo {
        version: manifest.version,
        notes: manifest.notes,
        url: manifest.url,
        sha256: manifest.sha256,
    };
    Ok(Some(info))
}

/// Download + verify + launch the installer, then quit so it can replace us.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    require_updates()?;
    let guard = InstallGuard::acquire().ok_or("正在下载更新，请稍候")?;
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

    // Same order as tauri-plugin-updater: start the installer, then quit. The
    // passive installer closes this app itself if it is still shutting down.
    installer_command(&installer)
        .spawn()
        .map_err(|e| format!("启动安装器失败：{e}"))?;
    guard.keep();
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod installer_tests {
    use super::*;

    #[test]
    fn installer_gets_update_switches_in_order() {
        let installer = Path::new("C:\\Temp\\VibeUsage-9.9.9-Setup.exe");
        let command = installer_command(installer);
        assert_eq!(command.get_program(), installer.as_os_str());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["/P", "/UPDATE", "/R"]);
    }

    fn update(version: &str) -> UpdateInfo {
        UpdateInfo {
            version: version.into(),
            notes: None,
            url: format!("https://example.test/VibeUsage-{version}-Setup.exe"),
            sha256: None,
        }
    }

    /// vibe-cafe/vibe-usage-windows#36: after a release is pulled, the next
    /// check finds no newer version; the stale offer and its download address
    /// must go, or 「发现更新」 keeps downloading the withdrawn installer.
    #[test]
    fn a_check_without_a_newer_version_drops_the_stale_update() {
        let slot = Mutex::new(None);
        assert!(!record_check(&slot, Some(&update("0.7.6"))));
        assert_eq!(slot.lock().unwrap().as_ref().unwrap().version, "0.7.6");

        assert!(record_check(&slot, None), "the earlier offer is withdrawn");
        assert!(slot.lock().unwrap().is_none());
        assert!(!record_check(&slot, None), "nothing left to withdraw");

        // A newer release replaces the old one rather than clearing it.
        record_check(&slot, Some(&update("0.7.6")));
        assert!(!record_check(&slot, Some(&update("0.7.7"))));
        let stored = slot.lock().unwrap().clone().unwrap();
        assert_eq!(stored.version, "0.7.7");
        assert!(stored.url.ends_with("VibeUsage-0.7.7-Setup.exe"));
    }

    #[test]
    fn second_install_is_refused_until_the_first_finishes() {
        let first = InstallGuard::acquire().expect("first install starts");
        assert!(InstallGuard::acquire().is_none());
        drop(first);
        assert!(InstallGuard::acquire().is_some());
    }

    /// Start a stand-in installer the way `install` does and record the exact
    /// command line it receives — the string NSIS reads into `$CMDLINE`.
    #[cfg(windows)]
    #[test]
    fn installer_receives_update_switches_through_a_path_with_spaces() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path().join("Vibe Usage update test");
        std::fs::create_dir(&dir).unwrap();
        let installer = dir.join("VibeUsage-9.9.9-Setup.exe");
        let source = temporary.path().join("echo.cs");
        let recorded = temporary.path().join("command-line.txt");
        std::fs::write(
            &source,
            "public static class Echo { public static void Main() { \
             System.IO.File.WriteAllText(System.Environment.GetEnvironmentVariable(\"VIBE_ECHO_OUT\"), \
             System.Environment.CommandLine); } }",
        )
        .unwrap();
        let built = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$env:PSModulePath = $PSHOME + '\\Modules'; \
                 Add-Type -Path $env:VIBE_ECHO_SRC -OutputAssembly $env:VIBE_ECHO_EXE -OutputType ConsoleApplication",
            ])
            .env("VIBE_ECHO_SRC", &source)
            .env("VIBE_ECHO_EXE", &installer)
            .status()
            .unwrap();
        assert!(built.success(), "compile the stand-in installer");

        let status = installer_command(&installer)
            .env("VIBE_ECHO_OUT", &recorded)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&recorded).unwrap(),
            format!("\"{}\" /P /UPDATE /R", installer.display())
        );
    }
}

#[cfg(all(test, feature = "external-test-diagnostics"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn external_build_rejects_manifest_before_network_access() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_millis(200))
            .build()
            .unwrap();
        let result = fetch_update(&client, "0.0.1", &url).await;
        assert_eq!(result.unwrap_err(), "外测版不检查或安装正式版更新");
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
