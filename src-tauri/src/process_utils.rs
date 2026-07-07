//! Windows process helpers (adapted from ATM's process_utils.rs):
//! spawn children without console-window flashes; kill whole process trees.

use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Hide the console window a spawned command would otherwise flash.
pub fn hide_command_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

pub fn hide_tokio_command_window(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Open a URL or file with the system default handler.
pub fn shell_open(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        // Keep shell/browser handler code out of the app process. Calling
        // ShellExecuteW directly can load third-party shell hooks into this
        // process; if one aborts, the whole tray app disappears even though the
        // browser launch may still complete.
        let mut cmd = Command::new("rundll32.exe");
        cmd.args(["url.dll,FileProtocolHandler", target]);
        hide_command_window(&mut cmd);
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("open failed: {e}"))
    }
    #[cfg(not(windows))]
    {
        open::that_detached(target).map_err(|e| e.to_string())
    }
}

/// Windows system proxy from the registry (Internet Settings), as an
/// `http://host:port` URL. None when disabled/unset or on other platforms.
pub fn system_proxy_url() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
            .ok()?;
        let enabled: u32 = key.get_value("ProxyEnable").ok()?;
        if enabled == 0 {
            return None;
        }
        let server: String = key.get_value("ProxyServer").ok()?;
        let server = server.trim();
        if server.is_empty() {
            return None;
        }
        // Either "host:port" (all protocols) or "http=h:p;https=h:p;...".
        let hostport = if server.contains('=') {
            server
                .split(';')
                .filter_map(|part| part.trim().split_once('='))
                .find(|(scheme, _)| *scheme == "https" || *scheme == "http")
                .map(|(_, hp)| hp.to_string())?
        } else {
            server.to_string()
        };
        Some(format!("http://{hostport}"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Kill a child and its whole descendant tree.
/// Windows `Child::kill` only terminates the direct child (e.g. node.exe would
/// survive a killed cmd.exe) — use `taskkill /T /F` instead.
pub fn kill_child_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let mut kill = Command::new("taskkill");
            kill.args(["/PID", &pid.to_string(), "/T", "/F"]);
            hide_command_window(&mut kill);
            let _ = kill.status();
            return;
        }
    }
    let _ = child.start_kill();
}
