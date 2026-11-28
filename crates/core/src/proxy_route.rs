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
