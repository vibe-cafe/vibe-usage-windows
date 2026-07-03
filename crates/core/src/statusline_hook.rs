//! Claude statusline capture hook — port of Services/StatuslineHook.swift.
//!
//! Installs a transparent Node wrapper into Claude Code's
//! `statusLine.command` so we can capture the `rate_limits` payload piped
//! there on every render. Install is idempotent and self-healing.

use crate::config::atomic_write;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const WRAPPER_JS: &str = include_str!("statusline/wrapper.js");

#[derive(Debug, Clone)]
pub struct StatuslineHook {
    pub claude_dir: PathBuf,
    pub vibe_dir: PathBuf,
    /// Absolute path of the Node executable to put in the command string.
    pub node_path: PathBuf,
}

#[derive(Debug)]
pub enum HookError {
    SettingsUnreadable(String),
    SettingsUnwritable(String),
}

impl std::fmt::Display for HookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookError::SettingsUnreadable(m) => write!(f, "无法读取 Claude 配置: {m}"),
            HookError::SettingsUnwritable(m) => write!(f, "无法写入 Claude 配置: {m}"),
        }
    }
}

impl std::error::Error for HookError {}

/// Honor CLAUDE_CONFIG_DIR (some users relocate ~/.claude), else default.
pub fn default_claude_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !custom.is_empty() {
            return expand_tilde(&custom);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

pub fn default_vibe_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vibe-usage")
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest.trim_start_matches(['/', '\\']));
        }
    }
    PathBuf::from(path)
}

impl StatuslineHook {
    pub fn new(node_path: PathBuf) -> Self {
        Self {
            claude_dir: default_claude_dir(),
            vibe_dir: default_vibe_dir(),
            node_path,
        }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.claude_dir.join("settings.json")
    }

    pub fn wrapper_path(&self) -> PathBuf {
        self.vibe_dir.join("vibe-usage-statusline.js")
    }

    pub fn sidecar_path(&self) -> PathBuf {
        self.vibe_dir.join("statusline-original")
    }

    pub fn backup_path(&self) -> PathBuf {
        self.vibe_dir.join("settings.json.vibe-bak")
    }

    pub fn rate_limit_file(&self) -> PathBuf {
        self.vibe_dir.join("claude-rate-limits.json")
    }

    /// The command we install into settings.json.
    pub fn wrapper_command(&self) -> String {
        format!(
            "\"{}\" \"{}\"",
            self.node_path.display(),
            self.wrapper_path().display()
        )
    }

    /// True when settings.json currently routes the statusline through our wrapper.
    pub fn is_installed(&self) -> bool {
        self.current_statusline_command().as_deref() == Some(self.wrapper_command().as_str())
    }

    /// Idempotently install (or re-assert) the wrapper. Safe to call repeatedly.
    pub fn install(&self) -> Result<(), HookError> {
        fs::create_dir_all(&self.vibe_dir)
            .map_err(|e| HookError::SettingsUnwritable(e.to_string()))?;
        atomic_write(&self.wrapper_path(), WRAPPER_JS.as_bytes())
            .map_err(|e| HookError::SettingsUnwritable(e.to_string()))?;

        let mut settings = self.load_settings()?;
        let wrapper_command = self.wrapper_command();
        let existing = settings
            .get("statusLine")
            .and_then(|s| s.get("command"))
            .and_then(Value::as_str)
            .map(String::from);

        // Capture the user's original command into the sidecar — but never
        // capture our own wrapper (that would chain the wrapper to itself).
        // An older wrapper command (different node path) is still ours: detect
        // by wrapper file path so a node upgrade doesn't self-chain either.
        if let Some(existing) = &existing {
            let is_ours = existing == &wrapper_command
                || existing.contains("vibe-usage-statusline");
            if !is_ours {
                self.backup_settings_if_needed();
                atomic_write(&self.sidecar_path(), existing.as_bytes())
                    .map_err(|e| HookError::SettingsUnwritable(e.to_string()))?;
            }
        }

        settings.insert(
            "statusLine".into(),
            json!({ "type": "command", "command": wrapper_command }),
        );
        self.save_settings(&settings)?;
        Ok(())
    }

    /// Restore the user's original statusLine command (from the sidecar).
    pub fn uninstall(&self) -> Result<(), HookError> {
        let mut settings = self.load_settings()?;
        let original = fs::read_to_string(self.sidecar_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        match original {
            Some(cmd) => {
                settings.insert(
                    "statusLine".into(),
                    json!({ "type": "command", "command": cmd }),
                );
            }
            None => {
                settings.remove("statusLine");
            }
        }
        self.save_settings(&settings)?;
        Ok(())
    }

    /// If capture was enabled but an external tool replaced
    /// `statusLine.command`, silently re-wrap: the replacement becomes the new
    /// "original" we forward to. No-op if already installed or never enabled.
    pub fn verify_and_repair(&self, enabled: bool) {
        if !enabled || self.is_installed() {
            return;
        }
        let _ = self.install();
    }

    fn current_statusline_command(&self) -> Option<String> {
        let settings = self.load_settings().ok()?;
        settings
            .get("statusLine")?
            .get("command")?
            .as_str()
            .map(String::from)
    }

    fn load_settings(&self) -> Result<Map<String, Value>, HookError> {
        let path = self.settings_path();
        if !path.exists() {
            return Ok(Map::new()); // No settings file yet — start fresh.
        }
        let raw =
            fs::read_to_string(&path).map_err(|e| HookError::SettingsUnreadable(e.to_string()))?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|e| HookError::SettingsUnreadable(e.to_string()))?;
        match value {
            Value::Object(map) => Ok(map),
            _ => Err(HookError::SettingsUnreadable(
                "settings.json is not a JSON object".into(),
            )),
        }
    }

    fn save_settings(&self, settings: &Map<String, Value>) -> Result<(), HookError> {
        fs::create_dir_all(&self.claude_dir)
            .map_err(|e| HookError::SettingsUnwritable(e.to_string()))?;
        let data = serde_json::to_string_pretty(&Value::Object(settings.clone()))
            .map_err(|e| HookError::SettingsUnwritable(e.to_string()))?;
        atomic_write(&self.settings_path(), data.as_bytes())
            .map_err(|e| HookError::SettingsUnwritable(e.to_string()))
    }

    /// One-time safety copy of the user's settings.json before our first edit.
    fn backup_settings_if_needed(&self) {
        let backup = self.backup_path();
        let settings = self.settings_path();
        if backup.exists() || !settings.exists() {
            return;
        }
        let _ = fs::copy(&settings, &backup);
    }
}

#[allow(dead_code)]
fn _assert_paths(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn hook(dir: &Path) -> StatuslineHook {
        StatuslineHook {
            claude_dir: dir.join("claude"),
            vibe_dir: dir.join("vibe-usage"),
            node_path: PathBuf::from("C:\\nodejs\\node.exe"),
        }
    }

    #[test]
    fn fresh_install_writes_wrapper_and_settings() {
        let dir = tempdir().unwrap();
        let h = hook(dir.path());
        h.install().unwrap();

        assert!(h.wrapper_path().exists());
        let wrapper = std::fs::read_to_string(h.wrapper_path()).unwrap();
        assert!(wrapper.contains("claude-rate-limits.json"));

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(h.settings_path()).unwrap()).unwrap();
        assert_eq!(
            settings["statusLine"]["command"].as_str().unwrap(),
            h.wrapper_command()
        );
        assert!(h.is_installed());
        // No prior statusline → no sidecar
        assert!(!h.sidecar_path().exists());
    }

    #[test]
    fn preserves_other_settings_and_captures_original() {
        let dir = tempdir().unwrap();
        let h = hook(dir.path());
        std::fs::create_dir_all(&h.claude_dir).unwrap();
        std::fs::write(
            h.settings_path(),
            r#"{"model":"opus","statusLine":{"type":"command","command":"npx claude-hud"},"permissions":{"allow":["Bash"]}}"#,
        )
        .unwrap();

        h.install().unwrap();

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(h.settings_path()).unwrap()).unwrap();
        assert_eq!(settings["model"], "opus", "unrelated keys preserved");
        assert_eq!(settings["permissions"]["allow"][0], "Bash");
        assert_eq!(
            std::fs::read_to_string(h.sidecar_path()).unwrap(),
            "npx claude-hud"
        );
        assert!(h.backup_path().exists(), "one-time backup created");
    }

    #[test]
    fn reinstall_never_chains_own_wrapper() {
        let dir = tempdir().unwrap();
        let h = hook(dir.path());
        std::fs::create_dir_all(&h.claude_dir).unwrap();
        std::fs::write(
            h.settings_path(),
            r#"{"statusLine":{"type":"command","command":"my-original-hud"}}"#,
        )
        .unwrap();
        h.install().unwrap();
        h.install().unwrap(); // idempotent second install
        assert_eq!(
            std::fs::read_to_string(h.sidecar_path()).unwrap(),
            "my-original-hud",
            "sidecar must still hold the true original"
        );

        // Node path changed (e.g. app update moved bundled node): old command
        // still contains vibe-usage-statusline → recognized as ours.
        let mut h2 = h.clone();
        h2.node_path = PathBuf::from("D:\\new\\node.exe");
        h2.install().unwrap();
        assert_eq!(
            std::fs::read_to_string(h2.sidecar_path()).unwrap(),
            "my-original-hud"
        );
        assert!(h2.is_installed());
    }

    #[test]
    fn uninstall_restores_original() {
        let dir = tempdir().unwrap();
        let h = hook(dir.path());
        std::fs::create_dir_all(&h.claude_dir).unwrap();
        std::fs::write(
            h.settings_path(),
            r#"{"statusLine":{"type":"command","command":"original-cmd"},"other":1}"#,
        )
        .unwrap();
        h.install().unwrap();
        h.uninstall().unwrap();

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(h.settings_path()).unwrap()).unwrap();
        assert_eq!(settings["statusLine"]["command"], "original-cmd");
        assert_eq!(settings["other"], 1);
    }

    #[test]
    fn uninstall_without_sidecar_removes_key() {
        let dir = tempdir().unwrap();
        let h = hook(dir.path());
        h.install().unwrap();
        h.uninstall().unwrap();
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(h.settings_path()).unwrap()).unwrap();
        assert!(settings.get("statusLine").is_none());
    }

    #[test]
    fn verify_and_repair_reasserts_after_clobber() {
        let dir = tempdir().unwrap();
        let h = hook(dir.path());
        h.install().unwrap();

        // External tool clobbers the statusline.
        std::fs::write(
            h.settings_path(),
            r#"{"statusLine":{"type":"command","command":"clobbering-hud"}}"#,
        )
        .unwrap();
        assert!(!h.is_installed());

        h.verify_and_repair(true);
        assert!(h.is_installed());
        // The clobbering command becomes the new "original" we forward to.
        assert_eq!(
            std::fs::read_to_string(h.sidecar_path()).unwrap(),
            "clobbering-hud"
        );

        // Disabled → repair is a no-op.
        std::fs::write(h.settings_path(), r#"{}"#).unwrap();
        h.verify_and_repair(false);
        assert!(!h.is_installed());
    }
}
