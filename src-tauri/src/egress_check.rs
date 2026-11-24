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
}

impl Pause {
    pub fn live_failure(self) -> RateLimitLiveFailure {
        match self {
            Self::RegionUnsupported(region) => RateLimitLiveFailure::EgressRegionUnsupported { region },
            Self::Unconfirmed => RateLimitLiveFailure::EgressUnconfirmed,
            Self::ProxyNotDetected => RateLimitLiveFailure::ProxyNotDetected,
        }
    }
}

/// The two settings (`AppSettings`): 「检测网络出口」 (default on; off sends a
/// direct read as before) and 「这台电脑上网靠本机代理软件」 (default off; on
/// pauses a direct read without a check).
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

/// The Anthropic host to check before Claude Code starts: the first one it
/// would reach direct, or none. `routes` are the live routes (the app's own
/// proxy variables and their `NO_PROXY`, which Claude Code inherits as they
/// are, then the system proxy). `claude_env` holds the proxy keys of Claude
/// Code's own `settings.json` `env`, which it applies on top (a proxy set only
/// there is common): with a proxy there, a host is direct only when the bypass
/// list names it; either way that list can send a host direct. The list is
/// read broadly (`bypass_matches`): reading a host as direct costs one check,
/// reading it as proxied would skip one.
pub fn claude_trace_host(routes: &[(String, Route)], claude_env: &HashMap<String, String>) -> Option<String> {
    let value = |names: &[&str]| {
        names
            .iter()
            .filter_map(|name| claude_env.get(*name))
            .find(|value| !value.trim().is_empty())
            .cloned()
    };
    let bypass = value(&["no_proxy", "NO_PROXY"]).unwrap_or_default();
    let own_proxy = value(&["https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"]).is_some();
    routes
        .iter()
        .find(|(host, route)| (!own_proxy && *route == Route::Direct) || bypass_matches(host, &bypass))
        .map(|(host, _)| host.clone())
}

/// A bypass list (`NO_PROXY`) read broadly, as any runtime might: `*`, an
/// exact host, or a domain and its subdomains (with or without a leading `.`
/// / `*.`); ports are ignored.
pub fn bypass_matches(host: &str, list: &str) -> bool {
    let host = host.to_ascii_lowercase();
    list.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|entry| !entry.is_empty())
        .any(|raw| {
            let mut entry = raw.to_ascii_lowercase();
            if entry == "*" {
                return true;
            }
            if !entry.starts_with('[') {
                if let Some(colon) = entry.rfind(':') {
                    entry.truncate(colon);
                }
            }
            let entry = entry.trim_start_matches("*.").trim_start_matches('.');
            !entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
        })
}

const PROXY_NAMES: [&str; 8] = [
    "HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy",
];

/// The proxy keys of Claude Code's own `settings.json` `env`
/// (`%CLAUDE_CONFIG_DIR%\settings.json`, else `~\.claude\settings.json`).
/// Only those keys are read; nothing is kept or logged.
fn claude_settings_proxy_env() -> HashMap<String, String> {
    let file = crate::services::diagnostics::claude_settings_file();
    let Some(env) = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|root| root.get("env").cloned())
    else {
        return HashMap::new();
    };
    PROXY_NAMES
        .iter()
        .filter_map(|name| env.get(*name).and_then(|value| value.as_str()).map(|value| (name.to_string(), value.to_string())))
        .collect()
}

/// Before Claude Code is started with the user's login.
pub async fn check_claude(settings: Settings) -> Result<(), Pause> {
    let (routes, mut claude_env) = tauri::async_runtime::spawn_blocking(|| {
        (
            crate::system_proxy::live_routes(&crate::system_proxy::host_urls(&crate::system_proxy::CLAUDE_HOSTS)),
            claude_settings_proxy_env(),
        )
    })
    .await
    .unwrap_or_default();
    // The app's own bypass list reaches Claude Code too, unless its settings
    // set one; read broadly here as well.
    if !["no_proxy", "NO_PROXY"].iter().any(|name| claude_env.get(*name).is_some_and(|v| !v.trim().is_empty())) {
        if let Some(own) = ["no_proxy", "NO_PROXY"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.trim().is_empty())
        {
            claude_env.insert("no_proxy".into(), own);
        }
    }
    match claude_trace_host(&routes, &claude_env) {
        Some(host) => require_direct(&host, settings, shared()).await,
        None => Ok(()),
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
    fn claude_is_checked_through_the_first_anthropic_host_that_goes_direct() {
        let proxied = Route::Http("http://127.0.0.1:7890".into());
        let routes = |direct: &[&str]| {
            ["api.anthropic.com", "platform.claude.com", "claude.ai"]
                .iter()
                .map(|host| {
                    let route = if direct.contains(host) { Route::Direct } else { proxied.clone() };
                    (host.to_string(), route)
                })
                .collect::<Vec<_>>()
        };
        let none = HashMap::new();
        assert_eq!(claude_trace_host(&routes(&[]), &none), None);
        assert_eq!(claude_trace_host(&routes(&["claude.ai"]), &none).as_deref(), Some("claude.ai"));
        assert_eq!(
            claude_trace_host(&routes(&["claude.ai", "api.anthropic.com"]), &none).as_deref(),
            Some("api.anthropic.com")
        );
        let unresolved = vec![("api.anthropic.com".to_string(), Route::Unresolved)];
        assert_eq!(claude_trace_host(&unresolved, &none), None);
        // A proxy set only in Claude Code's own settings.json is a proxy...
        let claude_proxy: HashMap<String, String> =
            [("HTTPS_PROXY".to_string(), "http://127.0.0.1:7890".to_string())].into();
        let all_direct = routes(&["api.anthropic.com", "platform.claude.com", "claude.ai"]);
        assert_eq!(claude_trace_host(&all_direct, &claude_proxy), None);
        // ...and its bypass list sends a host direct, past any proxy.
        let mut bypassed = claude_proxy.clone();
        bypassed.insert("no_proxy".into(), "claude.ai".into());
        assert_eq!(claude_trace_host(&routes(&[]), &bypassed).as_deref(), Some("claude.ai"));
        let only_bypass: HashMap<String, String> = [("NO_PROXY".to_string(), ".anthropic.com".to_string())].into();
        assert_eq!(claude_trace_host(&routes(&[]), &only_bypass).as_deref(), Some("api.anthropic.com"));
        assert!(bypass_matches("api.anthropic.com", "anthropic.com"));
        assert!(bypass_matches("api.anthropic.com", "localhost, *.anthropic.com:443"));
        assert!(bypass_matches("claude.ai", "*"));
        assert!(!bypass_matches("api.anthropic.com", "notanthropic.com"));
        // The app's own proxy variable with the user's NO_PROXY: Claude Code
        // goes direct to the listed host, so that host is checked.
        let settings = crate::system_proxy::ProxySettings::default();
        let env = |name: &str| match name {
            "HTTPS_PROXY" => Some("http://127.0.0.1:7890".to_string()),
            "NO_PROXY" => Some("api.anthropic.com".to_string()),
            _ => None,
        };
        let live = ["api.anthropic.com", "platform.claude.com", "claude.ai"]
            .iter()
            .map(|host| {
                let url = Url::parse(&format!("https://{host}/")).unwrap();
                (host.to_string(), crate::system_proxy::route_for(&url, &settings, &env, &|_, _| None))
            })
            .collect::<Vec<_>>();
        assert_eq!(claude_trace_host(&live, &none).as_deref(), Some("api.anthropic.com"));
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
