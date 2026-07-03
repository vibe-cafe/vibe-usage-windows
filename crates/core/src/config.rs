//! `~/.vibe-usage/config.json` IO — port of Models/Config.swift + AppConfig.swift.
//! Shared contract with the CLI: both read/write the same file.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const RELEASE_API_URL: &str = "https://vibecafe.ai";
pub const DEV_API_URL: &str = "http://localhost:3000";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VibeUsageConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<String>,
}

// The CLI writes camelCase keys; keep byte-compatible.
impl VibeUsageConfig {
    fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        if let Some(v) = &self.api_key {
            obj.insert("apiKey".into(), v.clone().into());
        }
        if let Some(v) = &self.api_url {
            obj.insert("apiUrl".into(), v.clone().into());
        }
        if let Some(v) = &self.hostname {
            obj.insert("hostname".into(), v.clone().into());
        }
        if let Some(v) = &self.last_sync {
            obj.insert("lastSync".into(), v.clone().into());
        }
        serde_json::Value::Object(obj)
    }

    fn from_json(v: &serde_json::Value) -> Self {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
        VibeUsageConfig {
            api_key: s("apiKey"),
            api_url: s("apiUrl"),
            hostname: s("hostname"),
            last_sync: s("lastSync"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigManager {
    pub config_dir: PathBuf,
    pub is_dev: bool,
}

impl ConfigManager {
    pub fn new(is_dev: bool) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            config_dir: home.join(".vibe-usage"),
            is_dev,
        }
    }

    pub fn with_dir(config_dir: PathBuf, is_dev: bool) -> Self {
        Self { config_dir, is_dev }
    }

    pub fn config_file_name(&self) -> &'static str {
        if self.is_dev {
            "config.dev.json"
        } else {
            "config.json"
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join(self.config_file_name())
    }

    pub fn state_path(&self) -> PathBuf {
        self.config_dir.join("state.json")
    }

    pub fn default_api_url(&self) -> &'static str {
        if self.is_dev {
            DEV_API_URL
        } else {
            RELEASE_API_URL
        }
    }

    pub fn load(&self) -> Option<VibeUsageConfig> {
        let raw = fs::read_to_string(self.config_path()).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        Some(VibeUsageConfig::from_json(&value))
    }

    pub fn save(&self, config: &VibeUsageConfig) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let data = serde_json::to_string_pretty(&config.to_json())?;
        atomic_write(&self.config_path(), data.as_bytes())
    }

    pub fn is_configured(&self) -> bool {
        self.load().and_then(|c| c.api_key).is_some()
    }

    /// 重置配置: delete config AND state.json so the CLI re-uploads from
    /// scratch on next link (state holds incremental-sync hashes; leaving it
    /// behind means a re-linked account would upload nothing).
    pub fn reset(&self) -> std::io::Result<()> {
        let _ = fs::remove_file(self.config_path());
        let _ = fs::remove_file(self.state_path());
        Ok(())
    }
}

/// Atomic write: temp file + rename. On Windows `rename` fails if the target
/// exists, so remove it first (delete-then-rename, same as ATM's config.rs).
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent dir"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    fs::write(&tmp, data)?;
    #[cfg(windows)]
    {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_camel_case_json() {
        let dir = tempdir().unwrap();
        let mgr = ConfigManager::with_dir(dir.path().to_path_buf(), false);
        let cfg = VibeUsageConfig {
            api_key: Some("vbu_test123".into()),
            api_url: Some("https://vibecafe.ai".into()),
            hostname: Some("my-pc".into()),
            last_sync: None,
        };
        mgr.save(&cfg).unwrap();

        let raw = std::fs::read_to_string(mgr.config_path()).unwrap();
        assert!(raw.contains("\"apiKey\""), "must write camelCase: {raw}");
        assert!(raw.contains("\"apiUrl\""));

        let loaded = mgr.load().unwrap();
        assert_eq!(loaded, cfg);
        assert!(mgr.is_configured());
    }

    #[test]
    fn reads_cli_written_config() {
        let dir = tempdir().unwrap();
        let mgr = ConfigManager::with_dir(dir.path().to_path_buf(), false);
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(
            mgr.config_path(),
            r#"{"apiKey":"vbu_abc","apiUrl":"https://vibecafe.ai","hostname":"host-1"}"#,
        )
        .unwrap();
        let cfg = mgr.load().unwrap();
        assert_eq!(cfg.api_key.as_deref(), Some("vbu_abc"));
        assert_eq!(cfg.hostname.as_deref(), Some("host-1"));
    }

    #[test]
    fn reset_removes_config_and_state() {
        let dir = tempdir().unwrap();
        let mgr = ConfigManager::with_dir(dir.path().to_path_buf(), false);
        mgr.save(&VibeUsageConfig {
            api_key: Some("vbu_x".into()),
            ..Default::default()
        })
        .unwrap();
        std::fs::write(mgr.state_path(), "{}").unwrap();
        mgr.reset().unwrap();
        assert!(!mgr.config_path().exists());
        assert!(!mgr.state_path().exists());
    }

    #[test]
    fn dev_mode_uses_dev_file() {
        let dir = tempdir().unwrap();
        let mgr = ConfigManager::with_dir(dir.path().to_path_buf(), true);
        assert_eq!(mgr.config_file_name(), "config.dev.json");
        assert_eq!(mgr.default_api_url(), DEV_API_URL);
    }
}
