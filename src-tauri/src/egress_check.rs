//! The egress check before a vendor-account read that goes out direct
//! (network-privacy principle, rule 3, workspace AGENTS.md / docs/NETWORK_PRIVACY.md;
//! maintenance plan 「代理与地区风险-修复方案」§3.4, decided by the maintainer
//! 2026-11-24). Same rules as macOS `EgressCheck`.
//!
//! When the live route of the vendor's host is a proxy (the app's own proxy
//! variable, the system proxy, a PAC answer), the request follows it
//! (`system_proxy`) and nothing here runs. When it is direct — no proxy, the
//! bypass list or a PAC `DIRECT`, or a proxy this computer cannot see (TUN, a
//! proxy on the router) — the app first asks the vendor's Cloudflare edge on
//! the same direct route which country it sees: an anonymous
//! `GET https://<host>/cdn-cgi/trace` with no cookie, token or account header,
//! of which only the two-letter `loc=` is read. A country the vendor does not
//! serve, or no answer, pauses the read: nothing carrying the account is sent
//! and the card says why. The next read checks again.
//!
//! The app checks Codex (`chatgpt.com`) and Claude (Anthropic's hosts, all with
//! a trace page). Antigravity is checked by the CLI, which gets the two
//! settings as environment variables (`Settings::cli_environment`).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reqwest::{StatusCode, Url};
use vibe_core::{EgressRegion, RateLimitLiveFailure};

use crate::system_proxy::Route;

/// Regions the paused vendors (OpenAI, Anthropic, xAI, Google) do not serve
/// and our users may be in.
pub const UNSUPPORTED_REGIONS: [&str; 3] = ["CN", "HK", "MO"];
pub const SUPPORTED_LIFETIME: Duration = Duration::from_secs(600);
pub const PAUSED_LIFETIME: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY_BYTES: usize = 4096;

/// The hosts Codex's usage URL may name that are OpenAI's own and have a
/// trace page. A `chatgpt_base_url` pointing elsewhere is a relay the user
/// chose: OpenAI sees the relay, not this computer.
pub const OPENAI_HOSTS: [&str; 2] = ["chatgpt.com", "chat.openai.com"];

/// Why a read was deliberately not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    RegionUnsupported(EgressRegion),
    Unconfirmed,
    ProxyNotDetected,
    /// The system sends an Anthropic host through a SOCKS proxy and Claude
    /// Code has no HTTP proxy it can use (`ClaudeRoute::SocksUnusable`): it
    /// would connect direct with the user's login (principle rules 1 and 2).
    /// A proxy was detected, so 「没检测到代理时」 does not apply; shown as
    /// 「无法确认网络出口」, e.g. when Claude's settings bypass the relay.
    SocksUnusable,
}

impl Pause {
    pub fn live_failure(self) -> RateLimitLiveFailure {
        match self {
            Self::RegionUnsupported(region) => RateLimitLiveFailure::EgressRegionUnsupported { region },
            Self::Unconfirmed | Self::SocksUnusable => RateLimitLiveFailure::EgressUnconfirmed,
            Self::ProxyNotDetected => RateLimitLiveFailure::ProxyNotDetected,
        }
    }
}

/// Settings → 隐私保护 → 「没检测到代理时」, one of three (the maintainer's
/// wording, 2026-11-25; same as macOS `EgressCheck.Mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EgressMode {
    /// 「自动判断（推荐）」, the default: check the egress first.
    #[default]
    Auto,
    /// 「暂停读取」: a direct read always pauses, and no check is sent.
    Pause,
    /// 「照常读取」: no check; a direct read is sent as before.
    Allow,
}

impl EgressMode {
    pub fn settings(self) -> Settings {
        Settings {
            enabled: self != EgressMode::Allow,
            require_proxy: self == EgressMode::Pause,
        }
    }

    /// The two switches of development builds (never released), for a
    /// settings file written before the menu: require-proxy on → 暂停读取,
    /// check off → 照常读取, else the default.
    pub fn from_legacy(object: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        if object.contains_key("egressMode") {
            return None;
        }
        let require = object.get("egressRequireLocalProxy").and_then(|v| v.as_bool());
        let enabled = object.get("egressCheckEnabled").and_then(|v| v.as_bool());
        match (require, enabled) {
            (Some(true), _) => Some(EgressMode::Pause),
            (_, Some(false)) => Some(EgressMode::Allow),
            (None, None) => None,
            _ => Some(EgressMode::Auto),
        }
    }
}

impl<'de> serde::Deserialize<'de> for EgressMode {
    /// A value this build does not know falls back to the default instead of
    /// failing the whole settings file.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value.as_str() {
            Some("pause") => EgressMode::Pause,
            Some("allow") => EgressMode::Allow,
            _ => EgressMode::Auto,
        })
    }
}

/// What `EgressMode` means for the checks (and, as two variables, for the
/// CLI): `enabled` off only for 「照常读取」, `require_proxy` on only for
/// 「暂停读取」.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub require_proxy: bool,
}

impl Settings {
    /// The same settings for the CLI, which checks Antigravity itself. Both
    /// are always written out, so a value the user set in their own
    /// environment for the terminal cannot override what Settings shows.
    pub fn cli_environment(self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("VIBE_USAGE_EGRESS_CHECK", if self.enabled { "1" } else { "0" }),
            ("VIBE_USAGE_REQUIRE_PROXY", if self.require_proxy { "1" } else { "0" }),
        ]
    }
}

/// `Err` when a read whose route is direct must not go out: the manual
/// setting, an unsupported country, or no answer. `Ok` when it may (also with
/// the check switched off).
pub async fn require_direct(trace_host: &str, settings: Settings, checker: &Checker) -> Result<(), Pause> {
    if settings.require_proxy {
        return Err(Pause::ProxyNotDetected);
    }
    if !settings.enabled {
        return Ok(());
    }
    match checker.region_through(trace_host).await {
        None => Err(Pause::Unconfirmed),
        Some(region) if UNSUPPORTED_REGIONS.contains(&region.as_str()) => {
            Err(Pause::RegionUnsupported(EgressRegion::from_code(&region)))
        }
        Some(_) => Ok(()),
    }
}

/// Before each Codex account request (every attempt, as the route is read
/// again for each one): checked only when that URL's live route is direct and
/// the URL is OpenAI's own.
pub async fn check_codex(url: &Url, settings: Settings) -> Result<(), Pause> {
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return Ok(());
    };
    if !OPENAI_HOSTS.contains(&host.as_str()) {
        return Ok(());
    }
    let target = url.clone();
    let route = tauri::async_runtime::spawn_blocking(move || crate::system_proxy::live_route(&target))
        .await
        .unwrap_or(Route::Unresolved);
    if route != Route::Direct {
        return Ok(());
    }
    require_direct(&host, settings, shared()).await
}

/// How Claude Code reaches Anthropic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeRoute {
    /// Every host it reaches goes through a proxy it can use.
    Proxied,
    /// This host goes direct under any reading of the bypass list: check it.
    Direct(String),
    /// Some host may go direct, but none surely does.
    Unsure,
    /// The system routes a host through SOCKS and Claude Code would not take
    /// an HTTP proxy to it (it has none, or its own bypass list may name that
    /// host), so it would connect direct. Not started, and not checked
    /// either: the anonymous check would go direct past the user's SOCKS proxy.
    SocksUnusable,
}

/// How Claude Code reaches Anthropic, judged by the environment it is
/// started with (the app's own proxy variables, or the system proxy for its
/// hosts with the ones a PAC sends direct in `NO_PROXY`, or the fail-closed
/// port) with the proxy keys of its own `settings.json` `env` on top (Claude
/// Code applies those over its environment; a proxy set only there is
/// common). Claude Code follows `HTTPS_PROXY` / `HTTP_PROXY` (either case),
/// no SOCKS or `ALL_PROXY`; with such a proxy a host goes direct only when the
/// bypass list (`NO_PROXY` and `no_proxy` together) names it. How Claude Code
/// reads an entry such as `.api.anthropic.com` or `*.anthropic.com` is not
/// settled (three cross-review rounds read its code differently), so both
/// ways count: a host that may go direct under any reading is not let through
/// unchecked, and the anonymous check goes only to a host that goes direct
/// under every reading, never to one the user's settings may proxy.
///
/// A SOCKS host must be reached through an HTTP proxy usable by Claude Code
/// (normally the probe's relay), including after settings are applied.
fn claude_launch_route(
    effective: &HashMap<String, String>, routes: &[(String, Route)], relay: Option<&str>,
) -> ClaudeRoute {
    let proxy = ["https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY"]
        .iter()
        .filter_map(|name| effective.get(*name))
        .map(|value| value.trim())
        .find(|value| !value.is_empty());
    let bypass = ["no_proxy", "NO_PROXY"]
        .iter()
        .filter_map(|name| effective.get(*name).cloned())
        .collect::<Vec<_>>()
        .join(",");
    // Claude Code takes only a full `http://` / `https://` URL: a `socks…://`
    // value, or a bare `host:port` copied from a proxy client, makes it
    // connect direct.
    let http_proxy = proxy.and_then(|v| Url::parse(v).ok())
        .is_some_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some());
    if proxy.is_some() && !http_proxy {
        return ClaudeRoute::SocksUnusable;
    }
    for (host, route) in routes {
        let needs_proxy = matches!(route, Route::Socks(_)) || (relay.is_some() && *route != Route::Direct);
        if needs_proxy && (!http_proxy || bypass_possibly(host, &bypass)) {
            return ClaudeRoute::SocksUnusable;
        }
    }
    if let Some(relay) = relay {
        if proxy == Some(relay) {
            // Even when settings remove NO_PROXY the relay honors the direct rule.
            if let Some((host, _)) = routes.iter().find(|(_, r)| *r == Route::Direct) {
                return ClaudeRoute::Direct(host.clone());
            }
        } else if routes.iter().any(|(_, r)| *r == Route::Unresolved) {
            return ClaudeRoute::SocksUnusable;
        }
    }
    let proxied = http_proxy;
    let hosts = crate::system_proxy::CLAUDE_HOSTS;
    if let Some(host) = hosts.iter().find(|host| !proxied || bypass_surely(host, &bypass)) {
        return ClaudeRoute::Direct(host.to_string());
    }
    if hosts.iter().any(|host| bypass_possibly(host, &bypass)) {
        ClaudeRoute::Unsure
    } else {
        ClaudeRoute::Proxied
    }
}

/// Entries of a bypass list that concern an https host: no port, or 443.
fn bypass_entries(list: &str) -> Vec<String> {
    list.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|entry| !entry.is_empty())
        .filter_map(|raw| {
            let mut entry = raw.to_ascii_lowercase();
            if !entry.starts_with('[') {
                if let Some(colon) = entry.rfind(':') {
                    if &entry[colon + 1..] != "443" {
                        return None;
                    }
                    entry.truncate(colon);
                }
            }
            (!entry.is_empty()).then_some(entry)
        })
        .collect()
}

/// Bypassed under every reading: `*`, or the host itself.
pub fn bypass_surely(host: &str, list: &str) -> bool {
    let host = host.to_ascii_lowercase();
    bypass_entries(list).iter().any(|entry| entry == "*" || *entry == host)
}

/// Bypassed under some reading: also a domain of it, with or without a
/// leading `.` / `*.`.
pub fn bypass_possibly(host: &str, list: &str) -> bool {
    let host = host.to_ascii_lowercase();
    bypass_entries(list).iter().any(|entry| {
        if entry == "*" {
            return true;
        }
        let domain = entry.strip_prefix("*.").or_else(|| entry.strip_prefix('.')).unwrap_or(entry);
        !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
    })
}

pub const PROXY_NAMES: [&str; 8] = [
    "HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy",
];

/// Claude Code's settings file as text, as Claude Code reads it: UTF-8 with
/// or without a byte-order mark, or little-endian UTF-16 with one (what
/// Windows PowerShell 5.1 writes).
pub fn settings_text(bytes: &[u8]) -> Option<String> {
    let utf16 = |rest: &[u8], little: bool| {
        let units = rest
            .chunks_exact(2)
            .map(|pair| if little { u16::from_le_bytes([pair[0], pair[1]]) } else { u16::from_be_bytes([pair[0], pair[1]]) })
            .collect::<Vec<_>>();
        String::from_utf16(&units).ok()
    };
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).ok(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        // Claude Code reads anything else as UTF-8 (big-endian UTF-16 too).
        _ => String::from_utf8(bytes.to_vec()).ok(),
    }
}

/// The proxy keys of Claude Code's own `settings.json` `env`
/// (`%CLAUDE_CONFIG_DIR%\settings.json`, else `~\.claude\settings.json`).
/// Only those keys are read; nothing is kept or logged.
fn claude_settings_proxy_env() -> HashMap<String, String> {
    let file = crate::services::diagnostics::claude_settings_file();
    let Some(env) = std::fs::read(&file)
        .ok()
        .and_then(|bytes| settings_text(&bytes))
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|root| root.get("env").cloned())
    else {
        return HashMap::new();
    };
    PROXY_NAMES
        .iter()
        .filter_map(|name| env.get(*name).and_then(|value| value.as_str()).map(|value| (name.to_string(), value.to_string())))
        .collect()
}

/// The proxy variables Claude Code is started with: the inherited ones
/// overlaid by the ones the app adds (`proxy_env`), as `probe_command` does.
pub fn claude_child(inherited: &impl Fn(&str) -> Option<String>, proxy_env: &[(&'static str, String)]) -> HashMap<String, String> {
    let mut child: HashMap<String, String> = PROXY_NAMES
        .iter()
        .filter_map(|name| inherited(name).map(|value| (name.to_string(), value)))
        .collect();
    child.extend(proxy_env.iter().map(|(name, value)| (name.to_string(), value.clone())));
    child
}

/// Before Claude Code is started with the user's login. `child` holds the
/// proxy variables it is started with (`claude_child`); `routes` the live
/// routes of its hosts.
pub async fn check_claude(settings: Settings, mut child: HashMap<String, String>, routes: &[(String, Route)], relay: Option<&str>) -> Result<(), Pause> {
    let claude_env = tauri::async_runtime::spawn_blocking(claude_settings_proxy_env).await.map_err(|_| Pause::Unconfirmed)?;
    overlay_claude_settings(&mut child, &claude_env)?;
    claude_decision(claude_launch_route(&child, routes, relay), settings, shared()).await
}

fn overlay_claude_settings(child: &mut HashMap<String, String>, claude_env: &HashMap<String, String>) -> Result<(), Pause> {
    // Windows variable names are case insensitive. If settings specify a
    // different spelling, don't let a stale inherited alias mask the change.
    for (name, value) in claude_env {
        if claude_env.get(&name.to_ascii_uppercase()).is_some_and(|other| other != value) {
            return Err(Pause::Unconfirmed);
        }
        child.insert(name.to_ascii_lowercase(), value.clone());
        child.insert(name.to_ascii_uppercase(), value.clone());
    }
    Ok(())
}


async fn claude_decision(route: ClaudeRoute, settings: Settings, checker: &Checker) -> Result<(), Pause> {
    match route {
        ClaudeRoute::Proxied => Ok(()),
        // Under every setting: a proxy was detected, Claude Code just cannot
        // follow it, and nothing (the check included) may go direct past it.
        ClaudeRoute::SocksUnusable => Err(Pause::SocksUnusable),
        ClaudeRoute::Direct(host) => require_direct(&host, settings, checker).await,
        // It may go direct, but no host is surely direct to check through.
        ClaudeRoute::Unsure if settings.require_proxy => Err(Pause::ProxyNotDetected),
        ClaudeRoute::Unsure if !settings.enabled => Ok(()),
        ClaudeRoute::Unsure => Err(Pause::Unconfirmed),
    }
}

// -- The check itself ----------------------------------------------------------

type FetchRegion = Box<dyn Fn(String) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

struct Entry {
    region: Option<String>,
    checked_at: Instant,
    network: String,
}

/// Answers per host, valid for one network (`network_fingerprint`): a
/// supported country for 10 minutes, anything that pauses for 1 minute, so a
/// proxy turned back on is seen by the next read. The lock is held across a
/// check, so callers arriving meanwhile share its answer.
pub struct Checker {
    entries: tokio::sync::Mutex<HashMap<String, Entry>>,
    fetch: FetchRegion,
    fingerprint: Box<dyn Fn() -> String + Send + Sync>,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl Checker {
    pub fn new(
        fetch: FetchRegion,
        fingerprint: Box<dyn Fn() -> String + Send + Sync>,
        now: Box<dyn Fn() -> Instant + Send + Sync>,
    ) -> Self {
        Self {
            entries: tokio::sync::Mutex::new(HashMap::new()),
            fetch,
            fingerprint,
            now,
        }
    }

    pub async fn region_through(&self, host: &str) -> Option<String> {
        let mut entries = self.entries.lock().await;
        let network = (self.fingerprint)();
        if let Some(entry) = entries.get(host) {
            let supported = entry
                .region
                .as_deref()
                .is_some_and(|region| !UNSUPPORTED_REGIONS.contains(&region));
            let lifetime = if supported { SUPPORTED_LIFETIME } else { PAUSED_LIFETIME };
            if entry.network == network && (self.now)().saturating_duration_since(entry.checked_at) < lifetime {
                return entry.region.clone();
            }
        }
        let region = (self.fetch)(host.to_string()).await;
        entries.insert(
            host.to_string(),
            Entry { region: region.clone(), checked_at: (self.now)(), network },
        );
        region
    }
}

fn shared() -> &'static Checker {
    static CHECKER: OnceLock<Checker> = OnceLock::new();
    CHECKER.get_or_init(|| {
        Checker::new(
            Box::new(|host| Box::pin(fetch_region(host))),
            Box::new(network_fingerprint),
            Box::new(Instant::now),
        )
    })
}

/// The two-letter country in a trace body, or `None` (unknown `XX`, Tor's
/// `T1`, absent).
pub fn trace_region(body: &str) -> Option<String> {
    let line = body.lines().find(|line| line.starts_with("loc="))?;
    let value = line["loc=".len()..].trim().to_ascii_uppercase();
    (value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_uppercase()) && value != "XX").then_some(value)
}

/// One anonymous trace request, direct: a client with every proxy off (the
/// environment's and the system's), no cookie store (reqwest keeps none
/// without its `cookies` feature), no redirect followed, 5 s, at most 4 KiB.
async fn fetch_region(host: String) -> Option<String> {
    #[cfg(debug_assertions)]
    if let Ok(forced) = std::env::var("VIBE_USAGE_EGRESS_TEST_REGION") {
        // UI acceptance builds: show a paused card without a network request.
        return (forced != "none").then_some(forced);
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .build()
        .ok()?;
    let mut response = client
        .get(format!("https://{host}/cdn-cgi/trace"))
        .header("Accept", "text/plain")
        .send()
        .await
        .ok()?;
    if response.status() != StatusCode::OK {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY_BYTES {
            return None;
        }
    }
    trace_region(&String::from_utf8_lossy(&body))
}

/// The computer's network addresses, hashed: joining another network or a
/// TUN adapter (Wintun, TAP) coming or going changes it, and with it every
/// cached answer. Loopback and IPv6 link-local addresses are left out.
pub fn network_fingerprint() -> String {
    fingerprint(adapter_addresses())
}

pub fn fingerprint(mut entries: Vec<String>) -> String {
    use sha2::{Digest, Sha256};
    entries.sort();
    let digest = Sha256::digest(entries.join("\n").as_bytes());
    digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
fn adapter_addresses() -> Vec<String> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_FRIENDLY_NAME,
        GAA_FLAG_SKIP_MULTICAST, IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6};

    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER | GAA_FLAG_SKIP_FRIENDLY_NAME;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64> = Vec::new();
    let mut ok = false;
    for _ in 0..3 {
        // u64 elements keep the buffer aligned for the structures inside.
        buffer = vec![0u64; (size as usize).div_ceil(8)];
        let result = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        if result == 0 {
            ok = true;
            break;
        }
        if result != ERROR_BUFFER_OVERFLOW {
            break;
        }
    }
    let mut entries = Vec::new();
    if !ok {
        return entries;
    }
    let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        let current = unsafe { &*adapter };
        if current.OperStatus == IfOperStatusUp && current.IfType != IF_TYPE_SOFTWARE_LOOPBACK {
            let mut unicast = current.FirstUnicastAddress;
            while !unicast.is_null() {
                let address = unsafe { &*unicast };
                let sockaddr = address.Address.lpSockaddr;
                if !sockaddr.is_null() {
                    let family = unsafe { (*sockaddr).sa_family };
                    let text = if family == AF_INET {
                        let v4 = unsafe { &*(sockaddr as *const SOCKADDR_IN) };
                        let raw = unsafe { v4.sin_addr.S_un.S_addr };
                        Some(std::net::Ipv4Addr::from(raw.to_ne_bytes()).to_string())
                    } else if family == AF_INET6 {
                        let v6 = unsafe { &*(sockaddr as *const SOCKADDR_IN6) };
                        let ip = std::net::Ipv6Addr::from(unsafe { v6.sin6_addr.u.Byte });
                        ((ip.segments()[0] & 0xffc0) != 0xfe80).then(|| ip.to_string())
                    } else {
                        None
                    };
                    if let Some(text) = text {
                        entries.push(format!("{}|{family}|{text}", current.IfType));
                    }
                }
                unicast = address.Next;
            }
        }
        adapter = current.Next;
    }
    entries
}

/// Development and test builds on other systems: no interfaces read, so only
/// the lifetimes apply.
#[cfg(not(windows))]
fn adapter_addresses() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Script {
        answer: Mutex<Option<String>>,
        calls: Mutex<Vec<String>>,
        network: Mutex<String>,
        clock: Mutex<Instant>,
    }

    fn scripted(answer: Option<&str>) -> (Arc<Script>, Checker) {
        let script = Arc::new(Script {
            answer: Mutex::new(answer.map(str::to_string)),
            calls: Mutex::new(Vec::new()),
            network: Mutex::new("net-a".into()),
            clock: Mutex::new(Instant::now()),
        });
        let (fetch, network, clock) = (script.clone(), script.clone(), script.clone());
        let checker = Checker::new(
            Box::new(move |host| {
                let script = fetch.clone();
                Box::pin(async move {
                    script.calls.lock().unwrap().push(host);
                    script.answer.lock().unwrap().clone()
                })
            }),
            Box::new(move || network.network.lock().unwrap().clone()),
            Box::new(move || *clock.clock.lock().unwrap()),
        );
        (script, checker)
    }

    const ON: Settings = Settings { enabled: true, require_proxy: false };

    #[test]
    fn the_menu_is_one_value_and_the_old_switches_map_onto_it() {
        assert_eq!(EgressMode::default(), EgressMode::Auto);
        assert_eq!(EgressMode::Auto.settings(), Settings { enabled: true, require_proxy: false });
        assert_eq!(EgressMode::Pause.settings(), Settings { enabled: true, require_proxy: true });
        assert_eq!(EgressMode::Allow.settings(), Settings { enabled: false, require_proxy: false });
        assert_eq!(serde_json::to_value(EgressMode::Pause).unwrap(), serde_json::json!("pause"));
        for (raw, mode) in [("\"auto\"", EgressMode::Auto), ("\"pause\"", EgressMode::Pause), ("\"allow\"", EgressMode::Allow),
            ("\"somethingNew\"", EgressMode::Auto), ("true", EgressMode::Auto), ("null", EgressMode::Auto)] {
            assert_eq!(serde_json::from_str::<EgressMode>(raw).unwrap(), mode, "{raw}");
        }
        let object = |json: serde_json::Value| json.as_object().unwrap().clone();
        assert_eq!(EgressMode::from_legacy(&object(serde_json::json!({}))), None);
        assert_eq!(EgressMode::from_legacy(&object(serde_json::json!({"egressCheckEnabled": false}))), Some(EgressMode::Allow));
        assert_eq!(
            EgressMode::from_legacy(&object(serde_json::json!({"egressCheckEnabled": false, "egressRequireLocalProxy": true}))),
            Some(EgressMode::Pause)
        );
        assert_eq!(
            EgressMode::from_legacy(&object(serde_json::json!({"egressCheckEnabled": true, "egressRequireLocalProxy": false}))),
            Some(EgressMode::Auto)
        );
        // Once the menu has been saved, its value wins.
        assert_eq!(
            EgressMode::from_legacy(&object(serde_json::json!({"egressMode": "allow", "egressRequireLocalProxy": true}))),
            None
        );
        // An unknown value keeps every other setting.
        let settings: crate::state::AppSettings =
            serde_json::from_str(r#"{"showTokensInTray": true, "egressMode": "somethingNew"}"#).unwrap();
        assert!(settings.show_tokens_in_tray);
        assert_eq!(settings.egress_mode, EgressMode::Auto);
    }

    #[test]
    fn trace_region_reads_only_a_two_letter_loc_line() {
        assert_eq!(trace_region("fl=1\nh=chatgpt.com\nip=203.0.113.9\nloc=US\ntls=TLSv1.3\n").as_deref(), Some("US"));
        assert_eq!(trace_region("loc=cn\r\n").as_deref(), Some("CN"));
        assert_eq!(trace_region("loc=XX\n"), None);
        assert_eq!(trace_region("loc=T1\n"), None);
        assert_eq!(trace_region("loc=USA\n"), None);
        assert_eq!(trace_region("colo=HKG\n"), None);
        assert_eq!(trace_region(""), None);
    }

    #[tokio::test]
    async fn a_direct_read_pauses_on_an_unsupported_or_unknown_egress() {
        for (answer, expected) in [
            (Some("US"), Ok(())),
            (Some("SG"), Ok(())),
            (Some("CN"), Err(Pause::RegionUnsupported(EgressRegion::CN))),
            (Some("HK"), Err(Pause::RegionUnsupported(EgressRegion::HK))),
            (Some("MO"), Err(Pause::RegionUnsupported(EgressRegion::MO))),
            (None, Err(Pause::Unconfirmed)),
        ] {
            let (script, checker) = scripted(answer);
            assert_eq!(require_direct("chatgpt.com", ON, &checker).await, expected, "{answer:?}");
            assert_eq!(*script.calls.lock().unwrap(), vec!["chatgpt.com".to_string()]);
        }
    }

    #[tokio::test]
    async fn the_two_settings_decide_without_a_check() {
        let (script, checker) = scripted(Some("CN"));
        let off = Settings { enabled: false, require_proxy: false };
        assert_eq!(require_direct("chatgpt.com", off, &checker).await, Ok(()));
        let manual = Settings { enabled: true, require_proxy: true };
        assert_eq!(require_direct("chatgpt.com", manual, &checker).await, Err(Pause::ProxyNotDetected));
        assert!(script.calls.lock().unwrap().is_empty());
        assert_eq!(
            off.cli_environment(),
            vec![("VIBE_USAGE_EGRESS_CHECK", "0"), ("VIBE_USAGE_REQUIRE_PROXY", "0")]
        );
        assert_eq!(
            manual.cli_environment(),
            vec![("VIBE_USAGE_EGRESS_CHECK", "1"), ("VIBE_USAGE_REQUIRE_PROXY", "1")]
        );
        assert_eq!(
            ON.cli_environment(),
            vec![("VIBE_USAGE_EGRESS_CHECK", "1"), ("VIBE_USAGE_REQUIRE_PROXY", "0")]
        );
    }

    #[tokio::test]
    async fn answers_are_kept_per_network_ten_minutes_when_supported_one_minute_otherwise() {
        let (script, checker) = scripted(Some("US"));
        let start = *script.clock.lock().unwrap();
        let at = |minutes: f64| *script.clock.lock().unwrap() = start + Duration::from_secs_f64(minutes * 60.0);
        let calls = || script.calls.lock().unwrap().len();

        assert_eq!(checker.region_through("chatgpt.com").await.as_deref(), Some("US"));
        at(9.9);
        assert_eq!(checker.region_through("chatgpt.com").await.as_deref(), Some("US"));
        assert_eq!(calls(), 1);
        // Another network (Wi-Fi switched, TUN adapter gone) asks again.
        *script.answer.lock().unwrap() = Some("CN".into());
        *script.network.lock().unwrap() = "net-b".into();
        assert_eq!(checker.region_through("chatgpt.com").await.as_deref(), Some("CN"));
        assert_eq!(calls(), 2);
        at(10.4);
        assert_eq!(checker.region_through("chatgpt.com").await.as_deref(), Some("CN"));
        assert_eq!(calls(), 2);
        // A paused answer lasts a minute: a proxy turned on is seen soon.
        *script.answer.lock().unwrap() = Some("US".into());
        at(11.0);
        assert_eq!(checker.region_through("chatgpt.com").await.as_deref(), Some("US"));
        assert_eq!(calls(), 3);
        at(21.5);
        assert_eq!(checker.region_through("chatgpt.com").await.as_deref(), Some("US"));
        assert_eq!(calls(), 4);
        assert_eq!(checker.region_through("api.anthropic.com").await.as_deref(), Some("US"));
        assert_eq!(script.calls.lock().unwrap().last().map(String::as_str), Some("api.anthropic.com"));
    }

    #[test]
    fn claude_is_judged_by_the_environment_it_really_gets() {
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        let proxy = "http://127.0.0.1:7890";
        let none: Vec<(String, Route)> = Vec::new();
        let direct = |host: &str| ClaudeRoute::Direct(host.to_string());
        assert_eq!(claude_launch_route(&env(&[]), &none, None), direct("api.anthropic.com"));
        assert_eq!(claude_launch_route(&env(&[("HTTPS_PROXY", proxy)]), &none, None), ClaudeRoute::Proxied);
        assert_eq!(claude_launch_route(&env(&[("http_proxy", proxy)]), &none, None), ClaudeRoute::Proxied);
        // Claude Code follows no ALL_PROXY of its own.
        assert_eq!(claude_launch_route(&env(&[("ALL_PROXY", "socks5://127.0.0.1:7891")]), &none, None), direct("api.anthropic.com"));
        // A host a PAC sends direct, added to NO_PROXY when Claude is started,
        // stays direct under a proxy from Claude's own settings.
        assert_eq!(
            claude_launch_route(&env(&[("NO_PROXY", "localhost,127.0.0.1,::1,claude.ai"), ("HTTPS_PROXY", proxy)]), &none, None),
            direct("claude.ai")
        );
        // Both spellings of the list count together.
        assert_eq!(
            claude_launch_route(&env(&[("HTTPS_PROXY", proxy), ("no_proxy", "localhost"), ("NO_PROXY", "api.anthropic.com")]), &none, None),
            direct("api.anthropic.com")
        );
        // Entries read differently by different runtimes: unsure, no check.
        for entry in [".api.anthropic.com", "*.anthropic.com", ".anthropic.com", "anthropic.com"] {
            assert_eq!(claude_launch_route(&env(&[("HTTPS_PROXY", proxy), ("NO_PROXY", entry)]), &none, None), ClaudeRoute::Unsure, "{entry}");
        }
        assert_eq!(claude_launch_route(&env(&[("HTTPS_PROXY", proxy), ("NO_PROXY", "api.anthropic.com:8443")]), &none, None), ClaudeRoute::Proxied);
        assert_eq!(claude_launch_route(&env(&[("HTTPS_PROXY", proxy), ("NO_PROXY", "*")]), &none, None), direct("api.anthropic.com"));
        // SOCKS hosts: no proxy Claude Code can use, so it is not started,
        // and not checked either, so the anonymous check never bypasses that
        // proxy. A `socks5://` value in its proxy variables is no way through.
        let socks = crate::system_proxy::CLAUDE_HOSTS
            .iter()
            .map(|host| (host.to_string(), Route::Socks("127.0.0.1:7891".into())))
            .collect::<Vec<_>>();
        assert_eq!(claude_launch_route(&env(&[]), &socks, None), ClaudeRoute::SocksUnusable);
        assert_eq!(claude_launch_route(&env(&[("HTTPS_PROXY", "socks5://127.0.0.1:7891")]), &socks, None), ClaudeRoute::SocksUnusable);
        assert_eq!(claude_launch_route(&env(&[("https_proxy", " SOCKS5H://127.0.0.1:7891 ")]), &socks, None), ClaudeRoute::SocksUnusable);
        assert_eq!(claude_launch_route(&env(&[("HTTP_PROXY", "127.0.0.1:7890")]), &socks, None), ClaudeRoute::SocksUnusable);
        assert_eq!(claude_launch_route(&env(&[("HTTPS_PROXY", proxy)]), &socks, None), ClaudeRoute::Proxied);
        assert_eq!(claude_launch_route(&env(&[("HTTPS_PROXY", "https://proxy.example:8443")]), &socks, None), ClaudeRoute::Proxied);
        assert!(bypass_surely("api.anthropic.com", "localhost, api.anthropic.com:443"));
        assert!(!bypass_surely("api.anthropic.com", ".anthropic.com"));
        assert!(bypass_possibly("api.anthropic.com", "anthropic.com"));
        assert!(bypass_possibly("api.anthropic.com", "*.anthropic.com"));
        assert!(bypass_possibly("api.anthropic.com", ".api.anthropic.com"));
        assert!(!bypass_possibly("api.anthropic.com", ".anthropic.com:8443"));
        assert!(!bypass_possibly("api.anthropic.com", ".notanthropic.com"));
    }

    /// What `claude_usage::fetch` hands Claude Code, built the same way: the
    /// system routes of its hosts (`settings`, with `pac` answering per host
    /// as WinHTTP does), the variables the app adds for them on top of the
    /// inherited ones, then the proxy keys of Claude Code's own `settings.json`.
    fn launched(
        settings: &crate::system_proxy::ProxySettings,
        pac: &dyn Fn(&str) -> Option<String>,
        inherited: &[(&str, &str)],
        claude_settings: &[(&str, &str)],
    ) -> ClaudeRoute {
        let lookup = |name: &str| inherited.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string());
        let routes: Vec<_> = crate::system_proxy::host_urls(&crate::system_proxy::CLAUDE_HOSTS)
            .into_iter().map(|url| {
                let r = crate::system_proxy::route_for(&url, settings, &lookup, &|_, target|
                    pac(Url::parse(target).unwrap().host_str().unwrap()));
                (url.host_str().unwrap().to_owned(), r)
            }).collect();
        let relay = routes.iter().any(|(_, r)| matches!(r, Route::Socks(_)))
            .then_some("http://vibe-usage:test@127.0.0.1:30123");
        let vars = match relay {
            Some(url) => crate::system_proxy::claude_relay_variables(url, &routes, &lookup),
            None => crate::system_proxy::proxy_variables(&routes, &lookup, false, false),
        };
        let mut child = claude_child(&lookup, &vars);
        overlay_claude_settings(&mut child, &claude_settings.iter().map(|(k,v)| (k.to_string(),v.to_string())).collect()).unwrap();
        claude_launch_route(&child, &routes, relay)
    }

    fn manual(server: &str, bypass: Option<&str>) -> crate::system_proxy::ProxySettings {
        crate::system_proxy::ProxySettings {
            proxy_enable: true,
            proxy_server: Some(server.into()),
            proxy_override: bypass.map(str::to_string),
            auto_config_url: None,
        }
    }

    fn pac_only() -> crate::system_proxy::ProxySettings {
        crate::system_proxy::ProxySettings {
            auto_config_url: Some("http://127.0.0.1:33331/pac".into()),
            ..Default::default()
        }
    }

    fn no_pac(_: &str) -> Option<String> {
        panic!("no PAC configured")
    }

    #[tokio::test]
    async fn relay_routes_only_check_a_host_pinned_direct() {
        let socks = manual("socks=127.0.0.1:7891", None);
        let (script, checker) = scripted(Some("US"));
        let all_socks = || launched(&socks, &no_pac, &[], &[]);
        assert_eq!(all_socks(), ClaudeRoute::Proxied);
        for mode in [EgressMode::Auto, EgressMode::Pause, EgressMode::Allow] {
            assert_eq!(claude_decision(all_socks(), mode.settings(), &checker).await, Ok(()));
        }
        assert!(script.calls.lock().unwrap().is_empty());
        let split = |host: &str| Some(match host {
            "claude.ai" => "",
            "platform.claude.com" => "127.0.0.1:7890",
            _ => "socks=127.0.0.1:7891",
        }.to_owned());
        // Even when Claude removes the injected bypass, the relay honors the
        // direct rule and only that host is eligible for the anonymous check.
        for settings in [&[][..], &[("no_proxy", "localhost")][..]] {
            let route = launched(&pac_only(), &split, &[], settings);
            assert_eq!(route, ClaudeRoute::Direct("claude.ai".into()));
            assert_eq!(claude_decision(route, ON, &checker).await, Ok(()));
        }
        assert_eq!(*script.calls.lock().unwrap(), ["claude.ai"]);
        assert_eq!(launched(&socks, &no_pac, &[("NO_PROXY", ".anthropic.com")], &[]),
            ClaudeRoute::Direct("api.anthropic.com".into()));
        assert_eq!(launched(&socks, &no_pac, &[("ALL_PROXY", "socks5h://127.0.0.1:7891")], &[]), ClaudeRoute::Proxied);
    }

    #[test]
    fn settings_cannot_send_a_proxied_host_direct_past_the_relay() {
        let socks = manual("socks=127.0.0.1:7891", None);
        for settings in [[("no_proxy", "*")], [("NO_PROXY", "api.anthropic.com:443")],
                         [("NO_PROXY", ".anthropic.com")], [("https_proxy", "socks5://127.0.0.1:7891")],
                         [("HTTPS_PROXY", "127.0.0.1:7890")]] {
            assert_eq!(launched(&socks, &no_pac, &[], &settings), ClaudeRoute::SocksUnusable);
        }
        assert_eq!(launched(&socks, &no_pac, &[], &[("HTTPS_PROXY", "http://127.0.0.1:7890")]), ClaudeRoute::Proxied);
        assert_eq!(launched(&socks, &no_pac, &[("HTTPS_PROXY", "http://127.0.0.1:7890")], &[]), ClaudeRoute::Proxied);
    }

    #[tokio::test]
    async fn claude_behind_an_unusable_socks_proxy_pauses_under_every_setting_without_a_check() {
        let (script, checker) = scripted(Some("US"));
        for settings in [EgressMode::Auto, EgressMode::Pause, EgressMode::Allow].map(EgressMode::settings) {
            assert_eq!(
                claude_decision(ClaudeRoute::SocksUnusable, settings, &checker).await,
                Err(Pause::SocksUnusable),
                "{settings:?}"
            );
        }
        assert!(script.calls.lock().unwrap().is_empty());
        assert_eq!(Pause::SocksUnusable.live_failure(), RateLimitLiveFailure::EgressUnconfirmed);
        // The other routes decide as before.
        assert_eq!(claude_decision(ClaudeRoute::Proxied, ON, &checker).await, Ok(()));
        assert_eq!(claude_decision(ClaudeRoute::Unsure, ON, &checker).await, Err(Pause::Unconfirmed));
        assert_eq!(claude_decision(ClaudeRoute::Direct("api.anthropic.com".into()), ON, &checker).await, Ok(()));
        assert_eq!(*script.calls.lock().unwrap(), vec!["api.anthropic.com".to_string()]);
    }

    #[test]
    fn claude_settings_are_read_in_the_encodings_windows_writes() {
        let json = r#"{"env":{"NO_PROXY":"claude.ai"}}"#;
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(json.as_bytes());
        let mut le = vec![0xFF, 0xFE];
        le.extend(json.encode_utf16().flat_map(u16::to_le_bytes));
        for bytes in [json.as_bytes().to_vec(), bom, le] {
            assert_eq!(settings_text(&bytes).as_deref(), Some(json));
        }
        // Big-endian UTF-16: not read, as Claude Code does not read it either.
        let mut be = vec![0xFE, 0xFF];
        be.extend(json.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(settings_text(&be), None);
    }

    #[test]
    fn a_pause_carries_its_own_reason() {
        assert_eq!(
            Pause::RegionUnsupported(EgressRegion::CN).live_failure(),
            RateLimitLiveFailure::EgressRegionUnsupported { region: EgressRegion::CN }
        );
        assert_eq!(Pause::Unconfirmed.live_failure(), RateLimitLiveFailure::EgressUnconfirmed);
        assert_eq!(Pause::ProxyNotDetected.live_failure(), RateLimitLiveFailure::ProxyNotDetected);
        assert!(RateLimitLiveFailure::EgressUnconfirmed.is_pause());
        assert!(!RateLimitLiveFailure::EndpointUnreachable.is_pause());
        assert_eq!(
            serde_json::to_value(RateLimitLiveFailure::EgressRegionUnsupported { region: EgressRegion::HK }).unwrap(),
            serde_json::json!({ "code": "egressRegionUnsupported", "region": "HK" })
        );
    }

    #[test]
    fn the_network_fingerprint_ignores_order_and_follows_the_addresses() {
        let base = fingerprint(vec!["6|2|192.168.1.20".into(), "6|23|2001:db8::1".into()]);
        assert_eq!(fingerprint(vec!["6|23|2001:db8::1".into(), "6|2|192.168.1.20".into()]), base);
        assert_ne!(
            fingerprint(vec!["6|2|192.168.1.20".into(), "6|23|2001:db8::1".into(), "53|2|198.18.0.1".into()]),
            base
        );
        assert_eq!(base.len(), 16);
    }
}
