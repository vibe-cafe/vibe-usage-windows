//! 「复制诊断信息」: the app's own half of the one-click diagnostics summary.
//!
//! Approved by the maintainer on 2026-11-09 under the architecture approval
//! gate (一键诊断方案修订二: "可以开工，你派活吧。 1、2、3我都同意。"). It is
//! built only when the user presses the button, stays on this machine, and is
//! copied to the clipboard by the user's own action.
//!
//! Whitelist only. Every field is an enum, a number, a boolean or one of our
//! fixed templates: no path or directory name, no hostname (a 4-character
//! hash instead), no environment-variable *value*, no proxy address, no key,
//! no Claude Code output and no CLI text. Variable names are reported only
//! when they look like ordinary names (`[A-Z0-9_]`); anything else is counted.
//! The frontend lays the report out and runs a final scrub over the text.
//!
//! It does not call the CLI (`doctor --json` comes later, as 2′), and it does
//! not touch the test-build diagnostics module.

use crate::services::claude_usage::{self, CandidateNow, ProbeRecord};
use crate::services::sync_engine;
use crate::services::sync_history::{self, KeyVerdict, SyncRecord};
use crate::state::{AppCtx, SyncStatus};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use tauri::{AppHandle, Manager};
use vibe_core::runtime::RuntimeKind;
use vibe_core::{
    ProviderRateLimit, RateLimitEmptyReason, RateLimitLiveFailure, RateLimitProvider,
    RateLimitSource, RateLimitStatus,
};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_NAMES: usize = 20;
const PROXY_VARIABLES: [&str; 3] = ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub schema_version: u32,
    /// Epoch milliseconds.
    pub generated_at: u64,
    pub app: AppInfo,
    pub system: SystemInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeInfo>,
    pub cli: CliInfo,
    pub account: AccountInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_tag: Option<String>,
    pub network: NetworkInfo,
    pub scheduler: SchedulerInfo,
    /// The app's own sync history, newest first.
    pub syncs: Vec<SyncRecord>,
    pub quotas: Vec<QuotaCard>,
    pub claude: ClaudeInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub build: BuildKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BuildKind {
    Release,
    ExternalTest,
    Development,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    /// 10 or 11, from the build number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_release: Option<u8>,
    /// `23H2`-style feature-update name, alphanumeric only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<u32>,
    pub architecture: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub kind: RuntimeName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeName {
    BundledNode,
    SystemNode,
    Bun,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub launcher_present: bool,
    /// Highest complete version in the launcher's cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub key_configured: bool,
    /// What the newest sync says about the key (only the newest record).
    pub last_sync_key: KeyVerdict,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub system_proxy: ProxyKind,
    /// The system proxy is handed to the sync CLI only when the app's own
    /// environment has no `HTTPS_PROXY`; the Claude probe never gets it.
    pub system_proxy_passed_to_sync: bool,
    /// An automatic proxy script (PAC, 「使用设置脚本」) is configured. Only
    /// its presence; the address is never read out.
    pub auto_config_script: bool,
    pub variables: Vec<ProxyVariable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyKind {
    None,
    /// Enabled, pointing at this machine (a local proxy client).
    Loopback,
    /// Enabled, pointing elsewhere.
    Remote,
    /// Enabled, but not a value the app can hand on.
    Unrecognized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyVariable {
    pub name: &'static str,
    /// Set as a user environment variable.
    pub user: bool,
    /// Set as a system environment variable.
    pub system: bool,
    /// Present in the app's own process environment.
    pub app: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerInfo {
    pub running: bool,
    pub syncing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCard {
    pub provider: RateLimitProvider,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<RateLimitSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_failure: Option<RateLimitLiveFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<RateLimitEmptyReason>,
    pub meter_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_as_of: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeInfo {
    pub selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_probe: Option<ProbeRecord>,
    pub candidates: Vec<CandidateNow>,
    /// File names of every `claude.*` on PATH (`claude.cmd`, `claude`, …).
    pub path_matches: Vec<String>,
    /// `ANTHROPIC*` / `CLAUDE*` user environment variables, names only.
    pub user_variables: VariableNames,
    pub settings: ClaudeSettings,
    pub config_dir_set: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableNames {
    pub names: Vec<String>,
    /// Names not shown: unusual characters, or beyond the first twenty.
    pub other: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClaudeSettings {
    Missing,
    Unreadable,
    /// Names of the keys under `env`, never their values.
    Read {
        env: VariableNames,
    },
}

pub async fn collect(app: &AppHandle) -> DiagnosticsReport {
    let blocking_app = app.clone();
    let mut report = tauri::async_runtime::spawn_blocking(move || collect_local(&blocking_app))
        .await
        .expect("diagnostics collection panicked");
    report.claude.candidates = claude_usage::candidates_with_versions().await;
    report
}

fn collect_local(app: &AppHandle) -> DiagnosticsReport {
    let ctx = app.state::<AppCtx>();
    let config_dir = ctx.config.config_dir.clone();
    let syncs = ctx
        .settings_path
        .parent()
        .map(|dir| sync_history::load(&sync_history::path_in(dir)))
        .unwrap_or_default();
    let sync_state = ctx.sync_state.lock().unwrap().clone();
    let scheduler_running = ctx.scheduler_task.lock().unwrap().is_some();
    let selected = ctx
        .settings
        .lock()
        .unwrap()
        .selected_quota_product_ids
        .clone();
    let quotas = {
        let cache = ctx.rate_limits.lock().unwrap();
        selected
            .iter()
            .map(|provider| quota_card(*provider, cache.snapshots.get(provider).map(|(s, _)| s)))
            .collect()
    };
    let environment = vibe_core::quota_product::DiscoveryEnvironment::live();
    let user_names = registry::environment_names(registry::Scope::User);
    let system_names = registry::environment_names(registry::Scope::System);
    let system_proxy = crate::process_utils::system_proxy_url();
    let app_has = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    let (windows_build, display_version) = registry::windows_version();

    DiagnosticsReport {
        schema_version: SCHEMA_VERSION,
        generated_at: now_ms(),
        app: AppInfo {
            version: plain_version(&app.package_info().version.to_string())
                .unwrap_or_else(|| "?".into()),
            build: if crate::state::IS_DEV {
                BuildKind::Development
            } else if cfg!(feature = "external-test-diagnostics") {
                BuildKind::ExternalTest
            } else {
                BuildKind::Release
            },
        },
        system: SystemInfo {
            windows_release: windows_build.map(windows_release),
            display_version: display_version.and_then(|value| short_token(&value)),
            build: windows_build,
            architecture: std::env::consts::ARCH,
        },
        runtime: sync_engine::detect_runtime(app).map(|runtime| RuntimeInfo {
            kind: match runtime.kind {
                RuntimeKind::BundledNode => RuntimeName::BundledNode,
                RuntimeKind::Node => RuntimeName::SystemNode,
                RuntimeKind::Bun => RuntimeName::Bun,
            },
            version: sync_engine::runtime_version(&runtime),
        }),
        cli: CliInfo {
            launcher_present: sync_engine::cli_entry(app).is_some(),
            cached_version: cached_cli_version(&config_dir.join("cli-cache")),
        },
        account: AccountInfo {
            key_configured: ctx.config.is_configured(),
            last_sync_key: sync_history::last_key_verdict(&syncs),
        },
        device_tag: gethostname::gethostname().to_str().map(device_tag),
        network: NetworkInfo {
            system_proxy: proxy_kind(system_proxy.as_deref(), registry::system_proxy_enabled()),
            system_proxy_passed_to_sync: system_proxy.is_some()
                && !app_has("HTTPS_PROXY")
                && !app_has("https_proxy"),
            auto_config_script: registry::auto_config_script_set(),
            variables: proxy_variables(&user_names, &system_names, app_has),
        },
        scheduler: SchedulerInfo {
            running: scheduler_running,
            syncing: sync_state.status == SyncStatus::Syncing,
            last_success_at: sync_state.last_sync_at,
        },
        syncs,
        quotas,
        claude: ClaudeInfo {
            selected: selected.contains(&RateLimitProvider::ClaudeCode),
            last_probe: claude_usage::last_probe(),
            candidates: Vec::new(),
            path_matches: vibe_core::claude_code::path_match_names(&environment),
            user_variables: variable_names(
                user_names
                    .iter()
                    .filter(|name| is_claude_related(name))
                    .cloned(),
            ),
            settings: claude_settings(&claude_settings_file()),
            config_dir_set: app_has("CLAUDE_CONFIG_DIR"),
        },
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn quota_card(provider: RateLimitProvider, snapshot: Option<&ProviderRateLimit>) -> QuotaCard {
    let Some(snapshot) = snapshot else {
        return QuotaCard {
            provider,
            status: if provider == RateLimitProvider::Cursor {
                "disabled"
            } else {
                "notRead"
            },
            source: None,
            live_failure: None,
            empty_reason: None,
            meter_count: 0,
            data_as_of: None,
        };
    };
    let windows = [snapshot.five_hour.is_some(), snapshot.seven_day.is_some()]
        .into_iter()
        .filter(|present| *present)
        .count();
    QuotaCard {
        provider,
        // The kind only: `Error`'s message is never copied.
        status: match snapshot.status {
            RateLimitStatus::Ok => "ok",
            RateLimitStatus::NoData => "noData",
            RateLimitStatus::Disabled => "disabled",
            RateLimitStatus::Unauthorized => "unauthorized",
            RateLimitStatus::RetryableError => "retryableError",
            RateLimitStatus::Error { .. } => "error",
        },
        source: snapshot.source,
        live_failure: snapshot.live_failure,
        empty_reason: snapshot.empty_reason,
        meter_count: snapshot.meters.len() + windows,
        data_as_of: snapshot.data_as_of,
    }
}

/// A short hash that tells one person's two machines apart without showing
/// the hostname (which often carries a real name).
fn device_tag(hostname: &str) -> String {
    let digest = Sha256::digest(hostname.trim().to_lowercase().as_bytes());
    format!("{:02x}{:02x}", digest[0], digest[1])
}

fn windows_release(build: u32) -> u8 {
    if build >= 22000 {
        11
    } else {
        10
    }
}

/// Digits and dots only (`0.7.5`), or nothing.
fn plain_version(raw: &str) -> Option<String> {
    let version = raw.trim().trim_start_matches('v');
    let valid = !version.is_empty()
        && version.len() <= 20
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    valid.then(|| version.to_string())
}

/// An alphanumeric token of at most eight characters (`23H2`), or nothing.
fn short_token(raw: &str) -> Option<String> {
    let token = raw.trim();
    (!token.is_empty() && token.len() <= 8 && token.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| token.to_string())
}

fn cached_cli_version(cache: &Path) -> Option<String> {
    let entries = std::fs::read_dir(cache).ok()?;
    entries
        .flatten()
        .filter(|entry| entry.path().join("complete").is_file())
        .filter_map(|entry| plain_version(&entry.file_name().to_string_lossy()))
        .max_by_key(|version| {
            version
                .split('.')
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
}

fn proxy_kind(url: Option<&str>, enabled: bool) -> ProxyKind {
    let Some(url) = url else {
        return if enabled {
            ProxyKind::Unrecognized
        } else {
            ProxyKind::None
        };
    };
    let host = url::Url::parse(url).ok().and_then(|parsed| {
        parsed
            .host_str()
            .map(|host| host.trim_matches(['[', ']']).to_ascii_lowercase())
    });
    match host {
        Some(host)
            if host == "localhost"
                || host == "::1"
                || host
                    .parse::<std::net::Ipv4Addr>()
                    .is_ok_and(|ip| ip.is_loopback()) =>
        {
            ProxyKind::Loopback
        }
        Some(_) => ProxyKind::Remote,
        None => ProxyKind::Unrecognized,
    }
}

fn proxy_variables(
    user: &[String],
    system: &[String],
    app_has: impl Fn(&str) -> bool,
) -> Vec<ProxyVariable> {
    let has =
        |names: &[String], wanted: &str| names.iter().any(|name| name.eq_ignore_ascii_case(wanted));
    PROXY_VARIABLES
        .iter()
        .map(|name| ProxyVariable {
            name,
            user: has(user, name),
            system: has(system, name),
            app: app_has(name) || app_has(&name.to_ascii_lowercase()),
        })
        .collect()
}

fn is_claude_related(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("ANTHROPIC") || upper.starts_with("CLAUDE")
}

/// Ordinary variable names (`[A-Z0-9_]`, at most 64 characters), sorted and
/// de-duplicated; anything else is only counted.
fn variable_names(names: impl IntoIterator<Item = String>) -> VariableNames {
    let mut shown = Vec::new();
    let mut other = 0;
    for name in names {
        let upper = name.to_ascii_uppercase();
        let ordinary = !upper.is_empty()
            && upper.len() <= 64
            && upper
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        if ordinary {
            shown.push(upper);
        } else {
            other += 1;
        }
    }
    shown.sort();
    shown.dedup();
    if shown.len() > MAX_NAMES {
        other += shown.len() - MAX_NAMES;
        shown.truncate(MAX_NAMES);
    }
    VariableNames {
        names: shown,
        other,
    }
}

fn claude_settings_file() -> std::path::PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
        .unwrap_or_else(|| std::path::PathBuf::from(".claude"))
        .join("settings.json")
}

fn claude_settings(file: &Path) -> ClaudeSettings {
    let Ok(bytes) = std::fs::read(file) else {
        return if file.exists() {
            ClaudeSettings::Unreadable
        } else {
            ClaudeSettings::Missing
        };
    };
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return ClaudeSettings::Unreadable;
    };
    let keys = root
        .get("env")
        .and_then(serde_json::Value::as_object)
        .map(|env| env.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    ClaudeSettings::Read {
        env: variable_names(keys),
    }
}

/// Read-only registry lookups: environment-variable *names* and the Windows
/// version. Values of environment variables are never read.
mod registry {
    #[derive(Clone, Copy)]
    pub enum Scope {
        User,
        System,
    }

    #[cfg(windows)]
    pub fn environment_names(scope: Scope) -> Vec<String> {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;
        let key = match scope {
            Scope::User => RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment"),
            Scope::System => RegKey::predef(HKEY_LOCAL_MACHINE)
                .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment"),
        };
        key.map(|key| key.enum_values().flatten().map(|(name, _)| name).collect())
            .unwrap_or_default()
    }

    #[cfg(not(windows))]
    pub fn environment_names(_scope: Scope) -> Vec<String> {
        Vec::new()
    }

    /// `(CurrentBuild, DisplayVersion)`.
    #[cfg(windows)]
    pub fn windows_version() -> (Option<u32>, Option<String>) {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
        else {
            return (None, None);
        };
        let build = key
            .get_value::<String, _>("CurrentBuild")
            .ok()
            .and_then(|value| value.trim().parse().ok());
        (build, key.get_value::<String, _>("DisplayVersion").ok())
    }

    #[cfg(not(windows))]
    pub fn windows_version() -> (Option<u32>, Option<String>) {
        (None, None)
    }

    /// Whether the user turned the system proxy on, even when its value is
    /// one the app cannot use.
    #[cfg(windows)]
    pub fn system_proxy_enabled() -> bool {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
            .and_then(|key| key.get_value::<u32, _>("ProxyEnable"))
            .is_ok_and(|enabled| enabled != 0)
    }

    #[cfg(not(windows))]
    pub fn system_proxy_enabled() -> bool {
        false
    }

    /// Whether 「使用设置脚本」 has a script address. The value itself is not
    /// returned.
    #[cfg(windows)]
    pub fn auto_config_script_set() -> bool {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
            .and_then(|key| key.get_value::<String, _>("AutoConfigURL"))
            .is_ok_and(|url| !url.trim().is_empty())
    }

    #[cfg(not(windows))]
    pub fn auto_config_script_set() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync_history::{SyncOutcome, SyncTrigger};

    /// Synthetic values that must never reach the report.
    const SECRETS: [&str; 12] = [
        "张三",
        "张 三",
        "My Projects",
        "-Users-zhangsan-Coding-secret-project",
        "vbu_test_0123456789abcdef",
        "sk-ant-api03-FAKEFAKEFAKE",
        "someone@example.com",
        "eyJhbGciOiJIUzI1NiJ9",
        "user:p%40ss",
        "10.0.0.5",
        "ZhangSan-PC",
        "secret-project",
    ];

    fn assert_clean(json: &str) {
        for secret in SECRETS {
            assert!(!json.contains(secret), "{secret} leaked into {json}");
        }
    }

    #[test]
    fn variable_names_keep_ordinary_names_and_only_count_the_rest() {
        let names = variable_names(
            [
                "ANTHROPIC_BASE_URL",
                "anthropic_auth_token",
                "CLAUDE_张三",
                "CLAUDE-sk-ant-api03-FAKEFAKEFAKE",
                "someone@example.com",
                "-Users-zhangsan-Coding-secret-project",
                "ANTHROPIC_BASE_URL",
            ]
            .map(String::from),
        );
        assert_eq!(names.names, ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"]);
        assert_eq!(names.other, 4);
        assert_clean(&serde_json::to_string(&names).unwrap());

        let many = variable_names((0..25).map(|i| format!("CLAUDE_{i:02}")));
        assert_eq!(many.names.len(), 20);
        assert_eq!(many.other, 5);
    }

    #[test]
    fn claude_settings_report_env_key_names_never_values() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("张 三").join("settings.json");
        assert_eq!(claude_settings(&file), ClaudeSettings::Missing);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            serde_json::json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-ant-api03-FAKEFAKEFAKE",
                    "HTTPS_PROXY": "http://user:p%40ss@10.0.0.5:8080",
                    "VIBE_KEY": "vbu_test_0123456789abcdef",
                    "-Users-zhangsan-Coding-secret-project": "1",
                    "someone@example.com": "eyJhbGciOiJIUzI1NiJ9.e30.sig"
                },
                "model": "my secret-project model"
            })
            .to_string(),
        )
        .unwrap();
        let settings = claude_settings(&file);
        assert_eq!(
            settings,
            ClaudeSettings::Read {
                env: VariableNames {
                    names: vec![
                        "ANTHROPIC_AUTH_TOKEN".into(),
                        "HTTPS_PROXY".into(),
                        "VIBE_KEY".into()
                    ],
                    other: 2,
                }
            }
        );
        assert_clean(&serde_json::to_string(&settings).unwrap());
        std::fs::write(&file, b"{broken").unwrap();
        assert_eq!(claude_settings(&file), ClaudeSettings::Unreadable);
    }

    #[test]
    fn proxy_is_classified_without_its_address() {
        assert_eq!(proxy_kind(None, false), ProxyKind::None);
        assert_eq!(proxy_kind(None, true), ProxyKind::Unrecognized);
        assert_eq!(
            proxy_kind(Some("http://127.0.0.1:7890"), true),
            ProxyKind::Loopback
        );
        assert_eq!(
            proxy_kind(Some("http://localhost:7890"), true),
            ProxyKind::Loopback
        );
        assert_eq!(
            proxy_kind(Some("http://[::1]:7890"), true),
            ProxyKind::Loopback
        );
        let remote = proxy_kind(Some("http://user:p%40ss@10.0.0.5:8080"), true);
        assert_eq!(remote, ProxyKind::Remote);
        assert_clean(&serde_json::to_string(&remote).unwrap());

        let variables = proxy_variables(
            &["https_proxy".into(), "Path".into()],
            &["ALL_PROXY".into()],
            |name| name == "http_proxy",
        );
        assert_eq!(
            variables,
            vec![
                ProxyVariable {
                    name: "HTTPS_PROXY",
                    user: true,
                    system: false,
                    app: false
                },
                ProxyVariable {
                    name: "HTTP_PROXY",
                    user: false,
                    system: false,
                    app: true
                },
                ProxyVariable {
                    name: "ALL_PROXY",
                    user: false,
                    system: true,
                    app: false
                },
            ]
        );
    }

    #[test]
    fn device_tag_is_four_hex_characters_and_stable() {
        let tag = device_tag("ZhangSan-PC");
        assert_eq!(tag.len(), 4);
        assert!(tag.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(tag, device_tag(" zhangsan-pc "));
        assert_ne!(tag, device_tag("Other-PC"));
        assert_clean(&tag);
    }

    #[test]
    fn versions_and_tokens_accept_only_their_plain_shape() {
        assert_eq!(plain_version("0.7.5").as_deref(), Some("0.7.5"));
        assert_eq!(plain_version("v22.23.1").as_deref(), Some("22.23.1"));
        assert_eq!(plain_version("-Users-zhangsan-Coding-secret-project"), None);
        assert_eq!(plain_version("0.14.9-beta"), None);
        assert_eq!(short_token("23H2").as_deref(), Some("23H2"));
        assert_eq!(short_token("张三的电脑"), None);
        assert_eq!(windows_release(22631), 11);
        assert_eq!(windows_release(19045), 10);

        let dir = tempfile::tempdir().unwrap();
        for (name, complete) in [
            ("0.14.9", true),
            ("0.14.10", true),
            ("0.15.0", false),
            ("-Users-zhangsan-Coding-secret-project", true),
            (".staging-1", true),
        ] {
            let version = dir.path().join(name);
            std::fs::create_dir_all(&version).unwrap();
            if complete {
                std::fs::write(version.join("complete"), name).unwrap();
            }
        }
        assert_eq!(cached_cli_version(dir.path()).as_deref(), Some("0.14.10"));
    }

    #[test]
    fn quota_cards_copy_codes_but_never_an_error_message() {
        let mut snapshot = ProviderRateLimit::empty(
            RateLimitProvider::KimiCode,
            RateLimitStatus::Error {
                message: "Bearer eyJhbGciOiJIUzI1NiJ9 someone@example.com".into(),
            },
        );
        snapshot.live_failure = Some(RateLimitLiveFailure::CliFailed);
        let card = quota_card(RateLimitProvider::KimiCode, Some(&snapshot));
        assert_eq!(card.status, "error");
        let json = serde_json::to_string(&card).unwrap();
        assert_clean(&json);
        assert!(json.contains("cliFailed"));
        assert_eq!(quota_card(RateLimitProvider::Grok, None).status, "notRead");
    }

    #[test]
    fn a_report_built_from_hostile_inputs_serializes_clean() {
        let report = DiagnosticsReport {
            schema_version: SCHEMA_VERSION,
            generated_at: 1,
            app: AppInfo {
                version: plain_version("0.7.5").unwrap(),
                build: BuildKind::Release,
            },
            system: SystemInfo {
                windows_release: Some(11),
                display_version: short_token("张三的电脑"),
                build: Some(22631),
                architecture: "x86_64",
            },
            runtime: Some(RuntimeInfo {
                kind: RuntimeName::BundledNode,
                version: plain_version("v22.23.1"),
            }),
            cli: CliInfo {
                launcher_present: true,
                cached_version: plain_version("-Users-zhangsan-Coding-secret-project"),
            },
            account: AccountInfo {
                key_configured: true,
                last_sync_key: KeyVerdict::Accepted,
            },
            device_tag: Some(device_tag("ZhangSan-PC")),
            network: NetworkInfo {
                system_proxy: proxy_kind(Some("http://user:p%40ss@10.0.0.5:8080"), true),
                system_proxy_passed_to_sync: true,
                auto_config_script: false,
                variables: proxy_variables(&["HTTPS_PROXY".into()], &[], |_| false),
            },
            scheduler: SchedulerInfo {
                running: true,
                syncing: false,
                last_success_at: Some(1),
            },
            syncs: vec![SyncRecord {
                started_at: 1,
                trigger: SyncTrigger::Manual,
                outcome: SyncOutcome::TimedOut,
                duration_ms: 120_000,
                reason: None,
            }],
            quotas: vec![],
            claude: ClaudeInfo {
                selected: true,
                last_probe: None,
                candidates: vec![],
                path_matches: vec!["claude.cmd".into()],
                user_variables: variable_names(
                    ["CLAUDE_张三", "ANTHROPIC_API_KEY"].map(String::from),
                ),
                settings: ClaudeSettings::Missing,
                config_dir_set: false,
            },
        };
        assert_clean(&serde_json::to_string(&report).unwrap());
    }

    /// One report touching every serialized shape the frontend lays out.
    fn contract_sample() -> DiagnosticsReport {
        use crate::services::claude_usage::{
            AnswerFields, CandidateNow, DesktopStep, ProbeAttempt, ProbeRecord, ProcessExit,
        };
        use crate::services::sync_history::{LauncherStage, SyncReason};
        use vibe_core::claude_code::{LaunchOrigin, LaunchVia, PathExtension};
        const AT: u64 = 1_794_205_920_000; // 2026-11-09 06:32 UTC
        let minute = 60_000;
        let sync = |ago: u64, trigger, outcome, duration_ms, reason| SyncRecord {
            started_at: AT - ago * minute,
            trigger,
            outcome,
            duration_ms,
            reason,
        };
        let card = |provider, status, source, live_failure, empty_reason, meter_count| QuotaCard {
            provider,
            status,
            source,
            live_failure,
            empty_reason,
            meter_count,
            data_as_of: Some((AT / 1000) as f64 - 1800.0),
        };
        let npm = LaunchOrigin::Path {
            position: 3,
            extension: PathExtension::Cmd,
        };
        DiagnosticsReport {
            schema_version: SCHEMA_VERSION,
            generated_at: AT,
            app: AppInfo {
                version: "0.7.5".into(),
                build: BuildKind::Release,
            },
            system: SystemInfo {
                windows_release: Some(11),
                display_version: Some("23H2".into()),
                build: Some(22631),
                architecture: "x86_64",
            },
            runtime: Some(RuntimeInfo {
                kind: RuntimeName::BundledNode,
                version: Some("22.23.1".into()),
            }),
            cli: CliInfo {
                launcher_present: true,
                cached_version: Some("0.14.9".into()),
            },
            account: AccountInfo {
                key_configured: true,
                last_sync_key: KeyVerdict::Undetermined,
            },
            device_tag: Some("7c2e".into()),
            network: NetworkInfo {
                system_proxy: ProxyKind::Loopback,
                system_proxy_passed_to_sync: true,
                auto_config_script: true,
                variables: proxy_variables(&["HTTPS_PROXY".into()], &["ALL_PROXY".into()], |_| {
                    false
                }),
            },
            scheduler: SchedulerInfo {
                running: true,
                syncing: false,
                last_success_at: Some(AT - 50 * minute),
            },
            syncs: vec![
                sync(
                    10,
                    SyncTrigger::Scheduled,
                    SyncOutcome::CliStartupFailed,
                    21_000,
                    Some(SyncReason::Launcher {
                        stage: LauncherStage::Network,
                    }),
                ),
                sync(
                    20,
                    SyncTrigger::Manual,
                    SyncOutcome::TimedOut,
                    120_000,
                    None,
                ),
                sync(
                    30,
                    SyncTrigger::Scheduled,
                    SyncOutcome::Failed,
                    3_000,
                    Some(SyncReason::CliExit { exit_code: 3 }),
                ),
                sync(
                    40,
                    SyncTrigger::Scheduled,
                    SyncOutcome::Failed,
                    2_000,
                    Some(SyncReason::ServerStatus { status: 502 }),
                ),
                sync(
                    45,
                    SyncTrigger::Manual,
                    SyncOutcome::Failed,
                    1_500,
                    Some(SyncReason::KeyRejected),
                ),
                sync(
                    50,
                    SyncTrigger::Scheduled,
                    SyncOutcome::Success,
                    18_000,
                    None,
                ),
            ],
            quotas: vec![
                card(
                    RateLimitProvider::ClaudeCode,
                    "ok",
                    Some(RateLimitSource::Cache),
                    Some(RateLimitLiveFailure::ClaudeTimedOut),
                    None,
                    1,
                ),
                card(
                    RateLimitProvider::Codex,
                    "noData",
                    Some(RateLimitSource::Live),
                    Some(RateLimitLiveFailure::EndpointHttpStatus { status: 502 }),
                    None,
                    0,
                ),
                card(
                    RateLimitProvider::OpenCodeGo,
                    "noData",
                    None,
                    None,
                    Some(RateLimitEmptyReason::NotEntitled),
                    0,
                ),
            ],
            claude: ClaudeInfo {
                selected: true,
                last_probe: Some(ProbeRecord {
                    at: AT - minute,
                    desktop: DesktopStep::NoRecord,
                    candidates_found: 2,
                    attempts: vec![
                        ProbeAttempt {
                            origin: LaunchOrigin::LocalBin,
                            via: LaunchVia::Direct,
                            initialize_ms: Some(900),
                            get_usage_ms: None,
                            failure: Some(RateLimitLiveFailure::ClaudeTimedOut),
                            exit: ProcessExit::EndedByApp,
                            answer: None,
                        },
                        ProbeAttempt {
                            origin: npm,
                            via: LaunchVia::ShimBinary,
                            initialize_ms: Some(1_200),
                            get_usage_ms: Some(2_300),
                            failure: Some(RateLimitLiveFailure::ClaudeNoWindows),
                            exit: ProcessExit::Exited { code: Some(1) },
                            answer: Some(AnswerFields {
                                rate_limits_available: Some(true),
                                five_hour: false,
                                seven_day: false,
                                plan: true,
                                error: true,
                            }),
                        },
                    ],
                }),
                candidates: vec![
                    CandidateNow {
                        origin: LaunchOrigin::LocalBin,
                        via: LaunchVia::Direct,
                        version: Some("2.1.282".into()),
                    },
                    CandidateNow {
                        origin: npm,
                        via: LaunchVia::ShimBinary,
                        version: Some("2.1.319".into()),
                    },
                ],
                path_matches: vec!["claude.exe".into(), "claude".into(), "claude.cmd".into()],
                user_variables: variable_names(["ANTHROPIC_BASE_URL".to_string()]),
                settings: ClaudeSettings::Read {
                    env: variable_names(["CLAUDE_CODE_USE_BEDROCK".to_string()]),
                },
                config_dir_set: true,
            },
        }
    }

    /// The frontend formats exactly this file (`tests/diagnostics.test.ts`),
    /// so a field Rust renames without TypeScript following breaks a test.
    /// Regenerate with `VIBE_UPDATE_FIXTURES=1 cargo test -p vibe-usage-app`.
    #[test]
    fn contract_fixture_matches_what_rust_serializes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/diagnostics-report.json");
        let actual = serde_json::to_value(contract_sample()).unwrap();
        if std::env::var_os("VIBE_UPDATE_FIXTURES").is_some() {
            let mut text = serde_json::to_string_pretty(&actual).unwrap();
            text.push('\n');
            std::fs::write(&path, text).unwrap();
        }
        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            stored, actual,
            "tests/fixtures/diagnostics-report.json is out of date; regenerate it"
        );
    }
}
