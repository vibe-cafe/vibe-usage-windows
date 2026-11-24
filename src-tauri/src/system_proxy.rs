//! The user's Windows proxy settings, resolved for one URL at a time and read
//! again for every request (network-privacy principle, rule 1: follow the
//! user's proxy, PAC included, in the processes we start too). Mirrors macOS
//! `SystemProxy`.
//!
//! Before this, the app's own reqwest client read the proxy once, when the app
//! started (so an app that started before the proxy client kept connecting
//! directly), ignored PAC, and turned a per-protocol `ProxyServer` into an
//! invalid proxy (direct). The CLI got the registry value on every run but no
//! PAC; Claude Code got nothing.
//!
//! Order, per URL:
//! 1. Proxy variables already in the app's environment (`HTTPS_PROXY`, …), as
//!    before.
//! 2. A PAC (`AutoConfigURL`), evaluated by WinHTTP (`WinHttpGetProxyForUrl`,
//!    3 s timeouts); the app never runs PAC JavaScript itself. The first
//!    proxy in the answer is the route; a `DIRECT` answer is the user's rule.
//!    When the PAC cannot be evaluated, the manual proxy below applies if it
//!    is on (what WinINet does), otherwise the route is `Unresolved`.
//! 3. The manual proxy (`ProxyEnable` + `ProxyServer`), honouring the bypass
//!    list (`ProxyOverride`, with `<local>`). A per-protocol list picks the
//!    entry for the URL's scheme (`https=` for https, falling back to
//!    `http=` as the CLI injection always did, then `socks=`).
//!
//! `Unresolved` fails closed: requests and children are pointed at a closed
//! local port (`FAIL_CLOSED_PROXY`), never sent direct; pausing with an
//! explanation is separate work.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Direct,
    /// An HTTP proxy URL (`http://host:port`, possibly with credentials).
    Http(String),
    /// A SOCKS proxy, `host:port`.
    Socks(String),
    Unresolved,
}

/// Where a request goes when its route cannot be resolved (a PAC that is set
/// but cannot be fetched or evaluated, typically while the proxy client is
/// restarting): a closed local port, so the request fails instead of going
/// direct. Never a real proxy.
pub const FAIL_CLOSED_PROXY: &str = "http://127.0.0.1:1";

impl Route {
    /// The proxy's `host:port`, for a reachability check; `None` when there is
    /// no proxy to check (direct, or a PAC that cannot be resolved yet).
    pub fn proxy_endpoint(&self) -> Option<(String, u16)> {
        let url = match self {
            Route::Http(url) => Url::parse(url).ok()?,
            Route::Socks(endpoint) => Url::parse(&format!("socks5://{endpoint}")).ok()?,
            Route::Direct | Route::Unresolved => return None,
        };
        let host = url.host_str()?.trim_matches(['[', ']']).to_string();
        Some((host, url.port_or_known_default().unwrap_or(1080)))
    }

    /// The proxy URL reqwest should use, or `None` for a direct connection.
    pub fn reqwest_proxy(&self) -> Option<String> {
        match self {
            Route::Http(url) => Some(url.clone()),
            Route::Socks(endpoint) => Some(format!("socks5h://{endpoint}")),
            Route::Unresolved => Some(FAIL_CLOSED_PROXY.to_string()),
            Route::Direct => None,
        }
    }
}

/// The registry values that decide a route (HKCU Internet Settings).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxySettings {
    pub proxy_enable: bool,
    pub proxy_server: Option<String>,
    pub proxy_override: Option<String>,
    pub auto_config_url: Option<String>,
}

pub fn read_settings() -> ProxySettings {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        else {
            return ProxySettings::default();
        };
        let text = |name: &str| {
            key.get_value::<String, _>(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        ProxySettings {
            proxy_enable: key.get_value::<u32, _>("ProxyEnable").map(|v| v != 0).unwrap_or(false),
            proxy_server: text("ProxyServer"),
            proxy_override: text("ProxyOverride"),
            auto_config_url: text("AutoConfigURL"),
        }
    }
    #[cfg(not(windows))]
    {
        ProxySettings::default()
    }
}

/// The proxy variables in the app's own environment, if any (HTTPS_PROXY,
/// then HTTP_PROXY, then ALL_PROXY; lowercase first, as Node and curl read).
pub fn env_proxy(lookup: &impl Fn(&str) -> Option<String>, https: bool) -> Option<String> {
    let names: &[&str] = if https {
        &["https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"]
    } else {
        &["http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"]
    };
    names
        .iter()
        .filter_map(|name| lookup(name))
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

/// The route for `url` under `settings`. `evaluate_pac(pac_url, target)` is
/// the WinHTTP answer: `Some("")` for direct, `Some(list)` for proxies,
/// `None` when it could not be evaluated.
pub fn route_for(
    url: &Url,
    settings: &ProxySettings,
    env: &impl Fn(&str) -> Option<String>,
    evaluate_pac: &impl Fn(&str, &str) -> Option<String>,
) -> Route {
    // The user's NO_PROXY applies whatever names the proxy (as with the old
    // reqwest system-proxy support): environment, PAC or manual settings.
    let no_proxy = ["no_proxy", "NO_PROXY"]
        .iter()
        .filter_map(|name| env(name))
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default();
    if url.host_str().is_some_and(|host| no_proxy_matches(host, &no_proxy)) {
        return Route::Direct;
    }
    let https = url.scheme() == "https";
    if let Some(value) = env_proxy(env, https) {
        return route_from_entry(&value);
    }
    if let Some(pac) = &settings.auto_config_url {
        if let Some(answer) = evaluate_pac(pac, url.as_str()) {
            return route_from_pac_answer(&answer);
        }
        if !settings.proxy_enable {
            return Route::Unresolved;
        }
    }
    if !settings.proxy_enable {
        return Route::Direct;
    }
    let Some(server) = settings.proxy_server.as_deref() else {
        return Route::Direct;
    };
    if let Some(host) = url.host_str() {
        if bypassed(host, settings.proxy_override.as_deref()) {
            return Route::Direct;
        }
    }
    route_from_proxy_server(server, https)
}

/// One proxy entry (`host:port`, `http://…`, `socks5://…`, `socks=host:port`).
fn route_from_entry(entry: &str) -> Route {
    let entry = entry.trim();
    if let Some((key, value)) = entry.split_once('=') {
        if !value.contains("://") && key.eq_ignore_ascii_case("socks") {
            return socks_route(value);
        }
    }
    if let Some((scheme, rest)) = entry.split_once("://") {
        if scheme.to_ascii_lowercase().starts_with("socks") {
            return socks_route(rest);
        }
    }
    match crate::process_utils::normalize_proxy_entry(entry) {
        Some(url) => Route::Http(url),
        None => Route::Unresolved,
    }
}

fn socks_route(endpoint: &str) -> Route {
    let endpoint = endpoint.trim().trim_end_matches('/');
    let valid = endpoint
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0));
    if valid {
        Route::Socks(endpoint.to_string())
    } else {
        Route::Unresolved
    }
}

/// `ProxyServer`: one value for every scheme, or `scheme=value;…`. A list
/// only when every item is `<known protocol>=…` (a `=` inside credentials does
/// not make one, as before). For https the `https=` entry, else `http=` (what
/// the CLI injection always did, and an HTTP proxy every child can use), else
/// `socks=`; for http `http=`, else `socks=`. The first entry that parses wins,
/// so an empty or broken `https=` falls back instead of breaking the route.
fn route_from_proxy_server(server: &str, https: bool) -> Route {
    const PROTOCOLS: [&str; 5] = ["http", "https", "ftp", "socks", "gopher"];
    let parts: Vec<&str> = server.split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
    let items: Vec<(&str, &str)> = parts.iter().filter_map(|p| p.split_once('=')).collect();
    let is_list = !parts.is_empty()
        && items.len() == parts.len()
        && items.iter().all(|(key, _)| PROTOCOLS.iter().any(|name| key.trim().eq_ignore_ascii_case(name)));
    if !is_list {
        return route_from_entry(server);
    }
    let order: &[&str] = if https { &["https", "http", "socks"] } else { &["http", "socks"] };
    for scheme in order {
        for (_, value) in items.iter().filter(|(key, _)| key.trim().eq_ignore_ascii_case(scheme)) {
            let route = if *scheme == "socks" { socks_route(value) } else { route_from_entry(value) };
            if route != Route::Unresolved {
                return route;
            }
        }
    }
    Route::Direct
}

/// curl-style `NO_PROXY` matching: `*`, an exact host or IP, a domain and its
/// subdomains (`example.com` and `.example.com` both match
/// `api.example.com`), or an IPv4 CIDR.
pub fn no_proxy_matches(host: &str, list: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    list.split([',', ' ']).map(str::trim).filter(|e| !e.is_empty()).any(|entry| {
        let entry = entry.to_ascii_lowercase();
        if entry == "*" {
            return true;
        }
        if let Some((network, bits)) = entry.split_once('/') {
            return ipv4_in_cidr(&host, network, bits);
        }
        // `*.example.com` and `.example.com`: subdomains only; a bare
        // `example.com`: the host itself and its subdomains (curl).
        if let Some(suffix) = entry.strip_prefix("*.").or_else(|| entry.strip_prefix('.')) {
            return host.ends_with(&format!(".{suffix}"));
        }
        let entry = entry.trim_matches(['[', ']']);
        host == entry || host.ends_with(&format!(".{entry}"))
    })
}

fn ipv4_in_cidr(host: &str, network: &str, bits: &str) -> bool {
    let (Ok(host), Ok(network), Ok(bits)) = (
        host.parse::<std::net::Ipv4Addr>(),
        network.parse::<std::net::Ipv4Addr>(),
        bits.parse::<u32>(),
    ) else {
        return false;
    };
    if bits > 32 {
        return false;
    }
    let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
    (u32::from(host) & mask) == (u32::from(network) & mask)
}

/// WinHTTP's answer: proxies separated by `;` or spaces, the first wins.
fn route_from_pac_answer(answer: &str) -> Route {
    match answer.split([';', ' ']).map(str::trim).find(|item| !item.is_empty()) {
        None => Route::Direct,
        Some(first) if first.eq_ignore_ascii_case("DIRECT") => Route::Direct,
        Some(first) => route_from_entry(first),
    }
}

/// `ProxyOverride`: `;`-separated host patterns with `*`, and `<local>` for
/// names without a dot.
pub fn bypassed(host: &str, override_list: Option<&str>) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    override_list
        .unwrap_or_default()
        .split(';')
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .any(|pattern| {
            if pattern == "<local>" {
                !host.contains('.') && !host.contains(':')
            } else {
                wildcard_match(&pattern, &host)
            }
        })
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if index == 0 {
            let Some(stripped) = rest.strip_prefix(part) else { return false };
            rest = stripped;
        } else if index == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(position) = rest.find(part) else { return false };
            rest = &rest[position + part.len()..];
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Live resolution (registry + WinHTTP), with a short PAC cache.

const PAC_CACHE: Duration = Duration::from_secs(60);
static PAC_ANSWERS: Mutex<Option<HashMap<(String, String), (Instant, Option<String>)>>> = Mutex::new(None);

fn cached_pac(pac: &str, target: &Url) -> Option<String> {
    // PAC answers depend on the host (and rarely the path); cache per host.
    let key = (
        pac.to_string(),
        format!("{}://{}:{}", target.scheme(), target.host_str().unwrap_or_default(), target.port_or_known_default().unwrap_or(0)),
    );
    if let Some((at, answer)) = PAC_ANSWERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(&key)
        .cloned()
    {
        if at.elapsed() < PAC_CACHE {
            return answer;
        }
    }
    let answer = evaluate_pac(pac, target.as_str());
    // Only answers are kept: a failure (the proxy client restarting) is
    // tried again on the next request instead of being remembered.
    if answer.is_some() {
        PAC_ANSWERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_or_insert_with(HashMap::new)
            .insert(key, (Instant::now(), answer.clone()));
    }
    answer
}

/// The route for `url` right now.
pub fn live_route(url: &Url) -> Route {
    let settings = read_settings();
    route_for(url, &settings, &|name| std::env::var(name).ok(), &|pac, _| cached_pac(pac, url))
}

#[cfg(windows)]
fn evaluate_pac(pac_url: &str, target: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpGetProxyForUrl, WinHttpOpen, WinHttpSetTimeouts,
        ERROR_WINHTTP_LOGIN_FAILURE, WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY,
        WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_PROXY_INFO,
    };
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
    unsafe fn read_wide(pointer: *const u16) -> String {
        if pointer.is_null() {
            return String::new();
        }
        let mut length = 0;
        while *pointer.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
    }
    let agent = wide("VibeUsage");
    let pac = wide(pac_url);
    let url = wide(target);
    unsafe {
        let session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        WinHttpSetTimeouts(session, 3000, 3000, 3000, 3000);
        // Anonymous first; the Windows login is offered only when the PAC
        // server asks for it (Microsoft's documented pattern).
        let mut options = WINHTTP_AUTOPROXY_OPTIONS {
            dwFlags: WINHTTP_AUTOPROXY_CONFIG_URL,
            dwAutoDetectFlags: 0,
            lpszAutoConfigUrl: pac.as_ptr(),
            lpvReserved: std::ptr::null_mut(),
            dwReserved: 0,
            fAutoLogonIfChallenged: 0,
        };
        let mut info: WINHTTP_PROXY_INFO = std::mem::zeroed();
        let mut ok = WinHttpGetProxyForUrl(session, url.as_ptr(), &mut options, &mut info) != 0;
        if !ok && windows_sys::Win32::Foundation::GetLastError() == ERROR_WINHTTP_LOGIN_FAILURE {
            options.fAutoLogonIfChallenged = 1;
            ok = WinHttpGetProxyForUrl(session, url.as_ptr(), &mut options, &mut info) != 0;
        }
        let answer = if !ok {
            None
        } else if info.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY {
            Some(String::new())
        } else if info.dwAccessType == WINHTTP_ACCESS_TYPE_NAMED_PROXY {
            Some(read_wide(info.lpszProxy))
        } else {
            None
        };
        if !info.lpszProxy.is_null() {
            GlobalFree(info.lpszProxy.cast());
        }
        if !info.lpszProxyBypass.is_null() {
            GlobalFree(info.lpszProxyBypass.cast());
        }
        WinHttpCloseHandle(session);
        answer
    }
}

#[cfg(not(windows))]
fn evaluate_pac(_pac_url: &str, _target: &str) -> Option<String> {
    None
}

// ---------------------------------------------------------------------------
// Environment for the processes the app starts.

/// Hosts the CLI reaches. A PAC can answer per host; one `HTTPS_PROXY`
/// cannot, so hosts the PAC sends direct join `NO_PROXY`.
pub const CLI_HOSTS: [&str; 11] = [
    "vibefriends.com",
    "registry.npmjs.org",
    "api.kimi.com",
    "auth.kimi.com",
    "open.bigmodel.cn",
    "api.z.ai",
    "opencode.ai",
    "cursor.com",
    "cli-chat-proxy.grok.com",
    "daily-cloudcode-pa.googleapis.com",
    "generativelanguage.googleapis.com",
];

/// Hosts the Claude Code probe reaches (Claude Code's network docs).
pub const CLAUDE_HOSTS: [&str; 3] = ["api.anthropic.com", "platform.claude.com", "claude.ai"];

pub const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// Routes for `urls`, read now; each keeps its own answer. A failed PAC is
/// not cached, so a later URL tries again.
pub fn live_routes(urls: &[Url]) -> Vec<(String, Route)> {
    let settings = read_settings();
    let env = |name: &str| std::env::var(name).ok();
    // One batch gets at most this long for PAC answers; what is left after it
    // is unresolved (and fails closed) instead of each URL waiting in turn.
    let deadline = Instant::now() + BATCH_PAC_BUDGET;
    urls.iter()
        .map(|url| {
            let route = route_for(url, &settings, &env, &|pac, _| {
                if Instant::now() < deadline { cached_pac(pac, url) } else { None }
            });
            (url.host_str().unwrap_or_default().to_string(), route)
        })
        .collect()
}

const BATCH_PAC_BUDGET: Duration = Duration::from_secs(6);

pub fn host_urls(hosts: &[&str]) -> Vec<Url> {
    hosts.iter().filter_map(|host| Url::parse(&format!("https://{host}/")).ok()).collect()
}

/// Whether the app's own environment names a proxy (then the system's is not
/// resolved for children; theirs wins).
pub fn has_own_proxy(lookup: &impl Fn(&str) -> Option<String>) -> bool {
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|name| lookup(name).is_some_and(|value| !value.trim().is_empty()))
}

/// The registry `cli-bootstrap.mjs` asks, read the same way: the last
/// `@vibe-cafe:registry`, else the last `registry`; quotes stripped; https
/// only; a registry the file holds credentials for (`//host/…:_authToken=`)
/// is skipped, as the launcher then uses the official one.
pub fn launcher_registry(npmrc: Option<&str>) -> Option<Url> {
    let text = npmrc?;
    let (mut scoped, mut plain) = (None, None);
    let mut credential_hosts: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        // An unquoted value ends at a `#` or `;` comment, as the launcher reads it.
        let value = value.trim();
        let value = if value.starts_with(['"', '\'']) { value } else { value.split(['#', ';']).next().unwrap_or("") };
        let (key, value) = (key.trim(), value.trim().trim_matches(['"', '\'']));
        if key == "@vibe-cafe:registry" {
            scoped = Some(value.to_string());
        } else if key == "registry" {
            plain = Some(value.to_string());
        } else if let Some(rest) = key.strip_prefix("//") {
            if key.contains(':') {
                if let Some(host) = rest.split(['/', ':']).next() {
                    credential_hosts.push(host.to_ascii_lowercase());
                }
            }
        }
    }
    let url = Url::parse(&scoped.or(plain)?).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    // The launcher never uses a registry URL that carries credentials.
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    (url.scheme() == "https" && !has_userinfo && !credential_hosts.contains(&host)).then_some(url)
}

/// Every URL the CLI reaches: the fixed hosts, plus the user's own registry
/// and server by their real URL (scheme and port matter to a PAC).
pub fn cli_urls(npmrc: Option<&str>, api_url: Option<&str>) -> Vec<Url> {
    let mut urls: Vec<Url> = CLI_HOSTS.iter().filter_map(|host| Url::parse(&format!("https://{host}/")).ok()).collect();
    if let Some(registry) = launcher_registry(npmrc) {
        urls.push(registry);
    }
    if let Some(url) = api_url.and_then(|value| Url::parse(value).ok()) {
        urls.push(url);
    }
    let mut seen: Vec<String> = Vec::new();
    urls.retain(|url| {
        let fresh = !seen.iter().any(|key| key == url.as_str());
        seen.push(url.as_str().to_string());
        fresh
    });
    urls
}

/// Variables to set on a child so it follows the system proxy, matching the
/// macOS app. Proxy variables already in the app's environment are kept as
/// they are (then only loopback and Node's switch are added). An HTTP proxy
/// gets `NODE_USE_ENV_PROXY=1` when `node_switch`; a SOCKS-only proxy goes as
/// `socks5://` without the switch (Node cannot follow SOCKS, and the CLI then
/// refuses requests carrying a vendor account instead of connecting
/// directly), and only where `allow_socks`.
pub fn proxy_variables(
    routes: &[(String, Route)],
    lookup: &impl Fn(&str) -> Option<String>,
    allow_socks: bool,
    node_switch: bool,
) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let own: Vec<(&'static str, String)> = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .chain(["ALL_PROXY", "all_proxy"].iter())
        .filter_map(|name| lookup(name).map(|value| (*name, value.trim().to_string())))
        .filter(|(_, value)| !value.is_empty())
        .collect();
    if !own.is_empty() {
        // The user's own proxy wins over the system's. Pick the proxy each
        // scheme really uses (the CLI's order: the scheme's own variable, then
        // ALL_PROXY, and for https an HTTP_PROXY last) and write those two out
        // explicitly, so Node's startup switch and older CLIs see what the CLI
        // itself would choose; a value without a scheme becomes `http://…`
        // (Node with the switch refuses to start on it). A SOCKS choice gets
        // no switch (overriding an inherited one): the CLI refuses vendor
        // requests it cannot route.
        let pick = |names: &[&str]| {
            names.iter().filter_map(|name| lookup(name)).map(|v| v.trim().to_string()).find(|v| !v.is_empty())
        };
        let https = pick(&["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY", "http_proxy", "HTTP_PROXY"]);
        let http = pick(&["http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"]);
        let is_socks = |value: &Option<String>| value.as_deref().is_some_and(|v| v.to_ascii_lowercase().starts_with("socks"));
        let with_scheme = |value: String| if value.contains("://") { value } else { format!("http://{value}") };
        if is_socks(&https) || is_socks(&http) {
            if node_switch {
                out.push(("NODE_USE_ENV_PROXY", "0".to_string()));
            }
        } else {
            // Lowercase spellings too: Node prefers them when both exist.
            if let Some(value) = https {
                let value = with_scheme(value);
                if lookup("https_proxy").is_some() {
                    out.push(("https_proxy", value.clone()));
                }
                out.push(("HTTPS_PROXY", value));
            }
            if let Some(value) = http {
                let value = with_scheme(value);
                if lookup("http_proxy").is_some() {
                    out.push(("http_proxy", value.clone()));
                }
                out.push(("HTTP_PROXY", value));
            }
            if node_switch {
                out.push(("NODE_USE_ENV_PROXY", "1".to_string()));
            }
        }
        push_no_proxy(&mut out, lookup, &[]);
        return out;
    }
    let http = most_common(routes.iter().filter_map(|(_, route)| match route {
        Route::Http(url) => Some(url.clone()),
        _ => None,
    }));
    let socks = most_common(routes.iter().filter_map(|(_, route)| match route {
        Route::Socks(endpoint) => Some(format!("socks5://{endpoint}")),
        _ => None,
    }));
    let unresolved = routes.iter().any(|(_, route)| *route == Route::Unresolved);
    let (proxy, is_http) = match (http, socks) {
        (Some(http), _) => (http, true),
        (None, Some(socks)) if allow_socks => {
            out.push(("ALL_PROXY", socks.clone()));
            // Override a switch the app itself may have inherited: Node's
            // https cannot follow SOCKS either way.
            if node_switch {
                out.push(("NODE_USE_ENV_PROXY", "0".to_string()));
            }
            (socks, false)
        }
        // A PAC that cannot be evaluated right now: a closed local port, so
        // the child fails instead of connecting directly.
        _ if unresolved => (FAIL_CLOSED_PROXY.to_string(), true),
        _ => {
            // Nothing to follow. Node's switch stays, as before: harmless
            // without a proxy, and it keeps an inherited proxy honoured.
            if node_switch {
                out.push(("NODE_USE_ENV_PROXY", "1".to_string()));
                push_no_proxy(&mut out, lookup, &[]);
            }
            return out;
        }
    };
    out.push(("HTTPS_PROXY", proxy.clone()));
    out.push(("HTTP_PROXY", proxy));
    if node_switch && is_http {
        out.push(("NODE_USE_ENV_PROXY", "1".to_string()));
    }
    let direct: Vec<&str> = routes
        .iter()
        .filter(|(_, route)| *route == Route::Direct)
        .map(|(host, _)| host.as_str())
        .collect();
    push_no_proxy(&mut out, lookup, &direct);
    out
}

/// The bypass list Node would use (lowercase `no_proxy` first; an empty value
/// counts as unset) plus loopback (`[::1]` too, as Node's fetch matches the
/// bracketed host) and `extra`, written to `NO_PROXY` and, when the app had
/// one, `no_proxy`. Left alone when it says `*`.
fn push_no_proxy(out: &mut Vec<(&'static str, String)>, lookup: &impl Fn(&str) -> Option<String>, extra: &[&str]) {
    let effective = ["no_proxy", "NO_PROXY"]
        .iter()
        .filter_map(|name| lookup(name))
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default();
    let mut entries: Vec<String> = Vec::new();
    for entry in effective.split([',', ' ']).map(str::trim) {
        if !entry.is_empty() && !entries.iter().any(|e| e == entry) {
            entries.push(entry.to_string());
        }
    }
    if entries.iter().any(|e| e == "*") {
        return;
    }
    // Loopback hosts the CLI must reach directly: it reads agy's quota (and
    // Antigravity's legacy history) from a local port, and under
    // NODE_USE_ENV_PROXY Node sends even 127.0.0.1 through HTTPS_PROXY unless
    // NO_PROXY lists it.
    for host in LOOPBACK.iter().chain(["[::1]"].iter()).chain(extra.iter()) {
        if !entries.iter().any(|e| e.eq_ignore_ascii_case(host)) {
            entries.push(host.to_string());
        }
    }
    let merged = entries.join(",");
    if lookup("no_proxy").is_some() {
        out.push(("no_proxy", merged.clone()));
    }
    out.push(("NO_PROXY", merged));
}

/// The most frequent value, the earliest on a tie.
fn most_common(values: impl Iterator<Item = String>) -> Option<String> {
    let values: Vec<String> = values.collect();
    let mut best: Option<(&String, usize)> = None;
    for value in &values {
        let count = values.iter().filter(|other| *other == value).count();
        if best.is_none_or(|(_, best_count)| count > best_count) {
            best = Some((value, count));
        }
    }
    best.map(|(value, _)| value.clone())
}

/// The longest a login launch waits for the user's proxy, and how often it
/// looks (`wait_for_launch_proxy`).
pub const LAUNCH_PROXY_WAIT_LIMIT: Duration = Duration::from_secs(120);
pub const LAUNCH_PROXY_POLL: Duration = Duration::from_secs(2);

/// Whether a TCP connection to `host:port` opens within a second.
pub fn accepts(host: &str, port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    addresses
        .take(4)
        .any(|address| TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok())
}

/// Polls `route` every `interval` until it names a proxy that `accepts` a
/// connection (true), or until `limit` (false). With no proxy yet it keeps
/// polling: many proxy clients set the system proxy only once they have
/// started, and whether this user relies on a proxy at all is not known here.
pub async fn wait_for_proxy<R, RF, A, AF>(
    limit: Duration,
    interval: Duration,
    mut route: R,
    mut accepts: A,
) -> bool
where
    R: FnMut() -> RF,
    RF: std::future::Future<Output = Route>,
    A: FnMut(String, u16) -> AF,
    AF: std::future::Future<Output = bool>,
{
    let started = tokio::time::Instant::now();
    loop {
        if let Some((host, port)) = route().await.proxy_endpoint() {
            if accepts(host, port).await {
                return true;
            }
        }
        if started.elapsed() >= limit {
            return false;
        }
        tokio::time::sleep(interval).await;
    }
}

/// A login launch's wait for the user's proxy: the live system route for
/// `url` (PAC included) must point at a proxy that accepts a connection, at
/// most `LAUNCH_PROXY_WAIT_LIMIT` (network-privacy principle, rules 1, 2 and
/// 6: the proxy client may not be up yet right after login).
pub async fn wait_for_launch_proxy(url: Url) -> bool {
    wait_for_proxy(
        LAUNCH_PROXY_WAIT_LIMIT,
        LAUNCH_PROXY_POLL,
        || {
            let url = url.clone();
            async move {
                tauri::async_runtime::spawn_blocking(move || live_route(&url))
                    .await
                    .unwrap_or(Route::Unresolved)
            }
        },
        |host, port| async move {
            tauri::async_runtime::spawn_blocking(move || accepts(&host, port))
                .await
                .unwrap_or(false)
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_proxy_route_names_the_endpoint_to_check() {
        assert_eq!(
            Route::Http("http://127.0.0.1:7890".into()).proxy_endpoint(),
            Some(("127.0.0.1".into(), 7890))
        );
        assert_eq!(
            Route::Http("http://user:pw@proxy.lan".into()).proxy_endpoint(),
            Some(("proxy.lan".into(), 80))
        );
        assert_eq!(
            Route::Socks("[::1]:1080".into()).proxy_endpoint(),
            Some(("::1".into(), 1080))
        );
        assert_eq!(Route::Direct.proxy_endpoint(), None);
        assert_eq!(Route::Unresolved.proxy_endpoint(), None);
    }

    #[tokio::test]
    async fn a_login_launch_goes_ahead_once_the_proxy_answers() {
        let routes = std::sync::Mutex::new(vec![
            Route::Direct,
            Route::Unresolved,
            Route::Http("http://127.0.0.1:7890".into()),
        ]);
        let up = wait_for_proxy(
            Duration::from_secs(5),
            Duration::from_millis(1),
            || {
                let mut list = routes.lock().unwrap();
                let next = if list.len() > 1 { list.remove(0) } else { list[0].clone() };
                async move { next }
            },
            |host, port| async move { host == "127.0.0.1" && port == 7890 },
        )
        .await;
        assert!(up);
    }

    #[tokio::test]
    async fn no_proxy_runs_out_the_limit_and_a_closed_port_does_not_count() {
        let started = std::time::Instant::now();
        let up = wait_for_proxy(
            Duration::from_millis(30),
            Duration::from_millis(5),
            || async { Route::Direct },
            |_, _| async { true },
        )
        .await;
        assert!(!up);
        assert!(started.elapsed() >= Duration::from_millis(30));
        assert!(!accepts("127.0.0.1", 1));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(accepts("127.0.0.1", listener.local_addr().unwrap().port()));
    }

    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn manual(server: &str, bypass: Option<&str>) -> ProxySettings {
        ProxySettings {
            proxy_enable: true,
            proxy_server: Some(server.into()),
            proxy_override: bypass.map(str::to_string),
            auto_config_url: None,
        }
    }

    fn no_pac(_: &str, _: &str) -> Option<String> {
        panic!("no PAC configured")
    }

    #[test]
    fn manual_proxy_single_value_and_disabled() {
        let target = url("https://chatgpt.com/backend-api/wham/usage");
        assert_eq!(
            route_for(&target, &manual("127.0.0.1:7890", None), &no_env, &no_pac),
            Route::Http("http://127.0.0.1:7890".into())
        );
        let mut off = manual("127.0.0.1:7890", None);
        off.proxy_enable = false;
        assert_eq!(route_for(&target, &off, &no_env, &no_pac), Route::Direct);
        assert_eq!(route_for(&target, &ProxySettings::default(), &no_env, &no_pac), Route::Direct);
    }

    /// The per-protocol form used to become an invalid proxy (direct).
    #[test]
    fn per_protocol_list_picks_the_entry_for_the_scheme() {
        let list = manual("http=127.0.0.1:7890;https=127.0.0.1:7892;socks=127.0.0.1:7891", None);
        assert_eq!(
            route_for(&url("https://cursor.com/"), &list, &no_env, &no_pac),
            Route::Http("http://127.0.0.1:7892".into())
        );
        assert_eq!(
            route_for(&url("http://example.test/"), &list, &no_env, &no_pac),
            Route::Http("http://127.0.0.1:7890".into())
        );
        let socks_only = manual("socks=127.0.0.1:7891", None);
        assert_eq!(
            route_for(&url("https://cursor.com/"), &socks_only, &no_env, &no_pac),
            Route::Socks("127.0.0.1:7891".into())
        );
        let http_only = manual("http=127.0.0.1:7890", None);
        assert_eq!(
            route_for(&url("https://cursor.com/"), &http_only, &no_env, &no_pac),
            Route::Http("http://127.0.0.1:7890".into())
        );
    }

    #[test]
    fn bypass_list_with_wildcards_and_local() {
        let settings = manual("127.0.0.1:7890", Some("*.bigmodel.cn;localhost;<local>;10.*"));
        for (target, expected) in [
            ("https://open.bigmodel.cn/", Route::Direct),
            ("http://localhost:3000/", Route::Direct),
            ("http://intranet/", Route::Direct),
            ("http://10.1.2.3/", Route::Direct),
            ("https://chatgpt.com/", Route::Http("http://127.0.0.1:7890".into())),
        ] {
            assert_eq!(route_for(&url(target), &settings, &no_env, &no_pac), expected, "{target}");
        }
    }

    /// PAC is evaluated by WinHTTP. The first proxy wins (no `DIRECT`
    /// fallback); a `DIRECT` answer is the user's rule; a PAC that cannot be
    /// evaluated falls back to the manual proxy when it is on, otherwise it
    /// is unresolved.
    #[test]
    fn pac_answers() {
        let pac = |answer: Option<&'static str>| ProxySettings {
            auto_config_url: Some("http://127.0.0.1:33331/pac".into()),
            ..ProxySettings::default()
        }
        .with_answer(answer);
        let target = url("https://chatgpt.com/");
        assert_eq!(pac(Some("127.0.0.1:7890;127.0.0.1:7891")).route(&target), Route::Http("http://127.0.0.1:7890".into()));
        assert_eq!(pac(Some("")).route(&target), Route::Direct);
        assert_eq!(pac(Some("socks=127.0.0.1:7891")).route(&target), Route::Socks("127.0.0.1:7891".into()));
        assert_eq!(pac(None).route(&target), Route::Unresolved);
        assert_eq!(Route::Unresolved.reqwest_proxy().as_deref(), Some(FAIL_CLOSED_PROXY));
        let mut both = pac(None);
        both.settings.proxy_enable = true;
        both.settings.proxy_server = Some("127.0.0.1:7890".into());
        assert_eq!(both.route(&target), Route::Http("http://127.0.0.1:7890".into()));
    }

    struct WithAnswer {
        settings: ProxySettings,
        answer: Option<&'static str>,
    }
    impl ProxySettings {
        fn with_answer(self, answer: Option<&'static str>) -> WithAnswer {
            WithAnswer { settings: self, answer }
        }
    }
    impl WithAnswer {
        fn route(&self, target: &Url) -> Route {
            let answer = self.answer;
            route_for(target, &self.settings, &no_env, &|_, _| answer.map(str::to_string))
        }
    }

    #[test]
    fn the_apps_own_proxy_variables_come_first() {
        let env = |name: &str| (name == "HTTPS_PROXY").then(|| "http://corp:8080".to_string());
        assert_eq!(
            route_for(&url("https://chatgpt.com/"), &manual("127.0.0.1:7890", None), &env, &no_pac),
            Route::Http("http://corp:8080".into())
        );
        let socks = |name: &str| (name == "ALL_PROXY").then(|| "socks5://127.0.0.1:7891".to_string());
        assert_eq!(
            route_for(&url("https://chatgpt.com/"), &ProxySettings::default(), &socks, &no_pac),
            Route::Socks("127.0.0.1:7891".into())
        );
    }

    #[test]
    fn reqwest_proxy_urls() {
        assert_eq!(Route::Http("http://h:1".into()).reqwest_proxy().as_deref(), Some("http://h:1"));
        assert_eq!(Route::Socks("h:1".into()).reqwest_proxy().as_deref(), Some("socks5h://h:1"));
        assert_eq!(Route::Direct.reqwest_proxy(), None);
        // Fail closed: an unresolvable PAC never means direct.
        assert_eq!(Route::Unresolved.reqwest_proxy().as_deref(), Some(FAIL_CLOSED_PROXY));
    }

    /// The baseline parser's cases, through the route the app now uses.
    #[test]
    fn baseline_proxy_server_cases_still_route() {
        let https = |server: &str| route_for(&url("https://chatgpt.com/"), &manual(server, None), &no_env, &no_pac);
        assert_eq!(https("http=127.0.0.1:7890;https= ;"), Route::Http("http://127.0.0.1:7890".into()));
        assert_eq!(https("http=127.0.0.1:7890;https=bad:port"), Route::Http("http://127.0.0.1:7890".into()));
        assert_eq!(https("user:p=ss@proxy.corp:8080"), Route::Http("http://user:p%3Dss@proxy.corp:8080".into()));
        // https without its own entry takes the HTTP entry first, as before.
        assert_eq!(https("http=127.0.0.1:7890;socks=127.0.0.1:7891"), Route::Http("http://127.0.0.1:7890".into()));
        assert_eq!(https("socks=127.0.0.1:7891"), Route::Socks("127.0.0.1:7891".into()));
    }

    #[test]
    fn no_proxy_applies_to_the_apps_own_proxy_variables() {
        let env = |name: &str| match name {
            "HTTP_PROXY" => Some("http://corp:8080".to_string()),
            "NO_PROXY" => Some("127.0.0.1,.corp.example,10.0.0.0/8".to_string()),
            _ => None,
        };
        let route = |target: &str| route_for(&url(target), &ProxySettings::default(), &env, &no_pac);
        assert_eq!(route("http://127.0.0.1:8080/backend-api/wham/usage"), Route::Direct);
        assert_eq!(route("https://api.corp.example/"), Route::Direct);
        assert_eq!(route("https://10.1.2.3/"), Route::Direct);
        assert_eq!(route("https://chatgpt.com/"), Route::Http("http://corp:8080".into()));
        assert!(no_proxy_matches("anything.example", "*"));
        assert!(!no_proxy_matches("notcorp.example", "corp.example"));
    }

    fn vars(routes: &[(&str, Route)], env: &[(&str, &str)], allow_socks: bool, node_switch: bool) -> Vec<(&'static str, String)> {
        let routes: Vec<(String, Route)> = routes.iter().map(|(h, r)| (h.to_string(), r.clone())).collect();
        let env: Vec<(String, String)> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        proxy_variables(&routes, &|name| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()), allow_socks, node_switch)
    }

    #[test]
    fn cli_variables_for_an_http_proxy_with_pac_direct_hosts() {
        let out = vars(
            &[
                ("cursor.com", Route::Http("http://127.0.0.1:7890".into())),
                ("open.bigmodel.cn", Route::Direct),
            ],
            &[("NO_PROXY", "corp.example")],
            true,
            true,
        );
        assert_eq!(
            out,
            vec![
                ("HTTPS_PROXY", "http://127.0.0.1:7890".into()),
                ("HTTP_PROXY", "http://127.0.0.1:7890".into()),
                ("NODE_USE_ENV_PROXY", "1".into()),
                ("NO_PROXY", "corp.example,localhost,127.0.0.1,::1,[::1],open.bigmodel.cn".into()),
            ]
        );
    }

    #[test]
    fn socks_goes_to_the_cli_without_the_switch_and_never_to_claude() {
        let routes = [("api.anthropic.com", Route::Socks("127.0.0.1:7891".into()))];
        let cli = vars(&routes, &[], true, true);
        assert!(cli.contains(&("HTTPS_PROXY", "socks5://127.0.0.1:7891".into())));
        assert!(cli.contains(&("ALL_PROXY", "socks5://127.0.0.1:7891".into())));
        assert!(cli.contains(&("NODE_USE_ENV_PROXY", "0".into())));
        assert!(vars(&routes, &[], false, false).is_empty());
    }

    /// A PAC that cannot be evaluated sends children to a closed port.
    #[test]
    fn an_unresolved_pac_fails_closed_for_children() {
        let out = vars(&[("cursor.com", Route::Unresolved), ("open.bigmodel.cn", Route::Direct)], &[], true, true);
        assert!(out.contains(&("HTTPS_PROXY", FAIL_CLOSED_PROXY.into())));
        assert!(out.contains(&("NO_PROXY", "localhost,127.0.0.1,::1,[::1],open.bigmodel.cn".into())));
    }

    #[test]
    fn an_http_proxy_alone_also_covers_https_for_children() {
        let out = vars(&[], &[("HTTP_PROXY", "127.0.0.1:7890")], true, true);
        assert!(out.contains(&("HTTP_PROXY", "http://127.0.0.1:7890".into())));
        assert!(out.contains(&("HTTPS_PROXY", "http://127.0.0.1:7890".into())));
    }

    #[test]
    fn nothing_to_follow_keeps_the_old_cli_variables() {
        assert_eq!(
            vars(&[("a", Route::Direct)], &[], true, true),
            vec![("NODE_USE_ENV_PROXY", "1".into()), ("NO_PROXY", "localhost,127.0.0.1,::1,[::1]".into())]
        );
        assert!(vars(&[("a", Route::Direct)], &[], false, false).is_empty());
    }

    #[test]
    fn the_users_scheme_less_proxy_gets_a_scheme_and_all_proxy_wins() {
        let out = vars(&[], &[("HTTPS_PROXY", "127.0.0.1:7890")], true, true);
        assert!(out.contains(&("HTTPS_PROXY", "http://127.0.0.1:7890".into())));
        assert!(out.contains(&("NODE_USE_ENV_PROXY", "1".into())));
        let all = vars(
            &[("cursor.com", Route::Direct), ("a", Route::Http("http://127.0.0.1:7890".into()))],
            &[("ALL_PROXY", "socks5://127.0.0.1:7891")],
            true,
            true,
        );
        assert_eq!(
            all,
            vec![("NODE_USE_ENV_PROXY", "0".into()), ("NO_PROXY", "localhost,127.0.0.1,::1,[::1]".into())]
        );
        assert!(has_own_proxy(&|name: &str| (name == "all_proxy").then(|| "x".to_string())));
    }

    #[test]
    fn the_launchers_registry_and_real_server_urls() {
        assert_eq!(
            launcher_registry(Some("registry=\"https://registry.npmmirror.com/\"\n")).map(|u| u.host_str().unwrap().to_string()),
            Some("registry.npmmirror.com".to_string())
        );
        assert_eq!(
            launcher_registry(Some("registry=https://a.example/\n@vibe-cafe:registry=https://b.example/\n"))
                .map(|u| u.host_str().unwrap().to_string()),
            Some("b.example".to_string())
        );
        assert_eq!(launcher_registry(Some("registry=https://npm.corp/\n//npm.corp/:_authToken=x\n")), None);
        assert_eq!(launcher_registry(Some("registry=http://plain.example/\n")), None);
        assert_eq!(launcher_registry(Some("registry=https://user:pass@npm.corp/\n")), None);
        assert_eq!(
            launcher_registry(Some("registry=https://m.example/ # mirror\n")).map(|u| u.host_str().unwrap().to_string()),
            Some("m.example".to_string())
        );
        let urls = cli_urls(Some("registry=https://registry.npmmirror.com/\n"), Some("http://usage.intra:8080"));
        assert!(urls.iter().any(|u| u.as_str() == "https://registry.npmmirror.com/"));
        assert!(urls.iter().any(|u| u.as_str() == "http://usage.intra:8080/"));
    }

    #[test]
    fn no_proxy_applies_to_system_proxies_and_wildcards_mean_subdomains() {
        let env = |name: &str| (name == "NO_PROXY").then(|| "127.0.0.1,*.chatgpt.com".to_string());
        let settings = manual("127.0.0.1:7890", None);
        assert_eq!(route_for(&url("http://127.0.0.1:8080/x"), &settings, &env, &no_pac), Route::Direct);
        assert_eq!(route_for(&url("https://api.chatgpt.com/"), &settings, &env, &no_pac), Route::Direct);
        // The root domain itself is not a subdomain.
        assert_eq!(
            route_for(&url("https://chatgpt.com/"), &settings, &env, &no_pac),
            Route::Http("http://127.0.0.1:7890".into())
        );
    }

    #[test]
    fn the_users_own_variables_per_scheme() {
        // HTTP proxies plus a SOCKS ALL_PROXY: both schemes have HTTP, so the switch is on.
        let mixed = vars(&[], &[("HTTP_PROXY", "http://h:1"), ("HTTPS_PROXY", "http://h:1"), ("ALL_PROXY", "socks5://h:2")], true, true);
        assert!(mixed.contains(&("NODE_USE_ENV_PROXY", "1".into())));
        // HTTP_PROXY plus a different HTTP ALL_PROXY: https takes ALL_PROXY, as the CLI does.
        let all = vars(&[], &[("HTTP_PROXY", "http://a:1"), ("ALL_PROXY", "http://b:2")], true, true);
        assert!(all.contains(&("HTTPS_PROXY", "http://b:2".into())));
        assert!(all.contains(&("HTTP_PROXY", "http://a:1".into())));
        // A SOCKS choice: nothing written, the switch off.
        let socks = vars(&[], &[("http_proxy", "http://a:1"), ("ALL_PROXY", "socks5://b:2")], true, true);
        assert!(socks.contains(&("NODE_USE_ENV_PROXY", "0".into())));
        assert!(!socks.iter().any(|(name, _)| *name == "HTTPS_PROXY"));
    }

    #[test]
    fn the_users_own_variables_are_kept_and_both_no_proxy_spellings_merge() {
        let out = vars(
            &[("a", Route::Http("http://127.0.0.1:7890".into()))],
            &[("https_proxy", "http://corp:8080"), ("no_proxy", "corp.invalid")],
            true,
            true,
        );
        assert_eq!(
            out,
            vec![
                ("https_proxy", "http://corp:8080".into()),
                ("HTTPS_PROXY", "http://corp:8080".into()),
                ("NODE_USE_ENV_PROXY", "1".into()),
                ("no_proxy", "corp.invalid,localhost,127.0.0.1,::1,[::1]".into()),
                ("NO_PROXY", "corp.invalid,localhost,127.0.0.1,::1,[::1]".into()),
            ]
        );
    }
}
