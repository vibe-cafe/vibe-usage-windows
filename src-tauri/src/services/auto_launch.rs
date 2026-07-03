//! Launch-at-login — Windows counterpart of SMAppService (registry Run key
//! via the `auto-launch` crate, no admin rights needed).

use auto_launch::AutoLaunchBuilder;

fn manager() -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    AutoLaunchBuilder::new()
        .set_app_name("Vibe Usage")
        .set_app_path(&exe.to_string_lossy())
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
