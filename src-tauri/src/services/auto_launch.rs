//! Launch-at-login — Windows counterpart of SMAppService (registry Run key
//! via the `auto-launch` crate, no admin rights needed).
//!
//! The Run value carries `--autostart`, so a login launch can wait for the
//! user's proxy before any quota read or sync (the proxy client may not be up
//! yet; network-privacy principle, workspace AGENTS.md). The window shows as
//! on any launch. A manual launch has no argument and does not wait.

use auto_launch::AutoLaunchBuilder;

pub const AUTOSTART_ARG: &str = "--autostart";
const APP_NAME: &str = "Vibe Usage";

fn manager() -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe.to_string_lossy())
        .set_args(&[AUTOSTART_ARG])
        .build()
        .map_err(|e| e.to_string())
}

pub fn get() -> Result<bool, String> {
    manager()?.is_enabled().map_err(|e| e.to_string())
}

pub fn set(enabled: bool) -> Result<(), String> {
    let m = manager()?;
    if enabled {
        m.enable().map_err(|e| e.to_string())
    } else {
        // Disabling when not enabled errors on some platforms — tolerate.
        match m.disable() {
            Ok(()) => Ok(()),
            Err(_) if !m.is_enabled().unwrap_or(false) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Whether this process was started by the Run value.
pub fn launched_at_login(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|arg| arg == AUTOSTART_ARG)
}

/// Registrations made before the argument existed: add it to the existing
/// Run value. Only that string is rewritten — never created, and the Task
/// Manager on/off entry (`StartupApproved`) is not touched, so a user who
/// switched autostart off there keeps it off (`enable()` would turn it on).
pub fn migrate_run_value() {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
        use winreg::RegKey;
        let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
            KEY_READ | KEY_SET_VALUE,
        ) else {
            return;
        };
        let Ok(current) = key.get_value::<String, _>(APP_NAME) else {
            return;
        };
        if let Some(updated) = with_autostart_arg(&current) {
            let _ = key.set_value(APP_NAME, &updated);
        }
    }
}

/// The Run value with `--autostart` appended, or `None` when it already has
/// it. The crate writes `<path> <args>`; an older value is `<path> `.
#[cfg_attr(not(windows), allow(dead_code))]
fn with_autostart_arg(current: &str) -> Option<String> {
    if current.split_whitespace().any(|part| part == AUTOSTART_ARG) {
        return None;
    }
    Some(format!("{} {}", current.trim_end(), AUTOSTART_ARG))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_run_value_gains_the_argument_once() {
        let old = r"C:\Users\a\AppData\Local\Vibe Usage\vibe-usage-app.exe ";
        let new = with_autostart_arg(old).unwrap();
        assert_eq!(new, r"C:\Users\a\AppData\Local\Vibe Usage\vibe-usage-app.exe --autostart");
        assert_eq!(with_autostart_arg(&new), None);
    }

    #[test]
    fn only_the_exact_argument_marks_a_login_launch() {
        assert!(launched_at_login(["app.exe".to_string(), "--autostart".to_string()]));
        assert!(!launched_at_login(["app.exe".to_string()]));
        assert!(!launched_at_login(["app.exe".to_string(), "--autostart=no".to_string()]));
    }
}
