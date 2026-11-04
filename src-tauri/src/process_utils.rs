//! Windows process helpers (adapted from ATM's process_utils.rs):
//! spawn children without console-window flashes; kill whole process trees.

use std::{path::Path, process::Command};

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

/// Open a URL with the system default handler.
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

/// Launch a trusted executable path we created ourselves, such as the verified
/// updater installer. This avoids shell handlers entirely.
pub fn launch_executable(path: &Path) -> Result<(), String> {
    let mut cmd = Command::new(path);
    hide_command_window(&mut cmd);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("launch failed: {e}"))
}

/// Windows system proxy from the registry (Internet Settings), as an
/// `http(s)://host:port` URL (see `parse_proxy_server`). None when
/// disabled/unset/unrecognized or on other platforms.
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
        let url = parse_proxy_server(server);
        if url.is_none() {
            // Never log the raw value: it may carry user:password.
            log::warn!("system proxy ignored: unrecognized ProxyServer format");
        }
        url
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Turn a Windows `ProxyServer` value into a proxy URL Node can use, or `None`
/// when the value is not something we can express safely. Accepted forms:
/// `host:port`, `http://host:port`, `https://host:port`, and the per-protocol
/// list `http=h:p;https=h:p;socks=h:p` (https entry wins, then http; each
/// entry may itself carry a scheme). `socks`/`ftp` entries and any other
/// scheme are unsupported by Node's env proxy, so they yield `None` rather
/// than a malformed URL.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_proxy_server(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Per-protocol list: the text before the first '=' is a bare protocol
    // name ("http=..."). A '=' inside "user:p=ss@host" has a ':' before it.
    let is_list = raw
        .split_once('=')
        .is_some_and(|(k, _)| !k.trim().is_empty() && k.trim().chars().all(|c| c.is_ascii_alphabetic()));
    if !is_list {
        return normalize_proxy_entry(raw);
    }
    // Try https first, then http; an empty/invalid https entry falls back.
    ["https", "http"].into_iter().find_map(|name| {
        raw.split(';')
            .filter_map(|part| part.trim().split_once('='))
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .find_map(|(_, v)| normalize_proxy_entry(v))
    })
}

fn normalize_proxy_entry(entry: &str) -> Option<String> {
    let entry = entry.trim();
    let (scheme, rest) = match entry.split_once("://") {
        Some((s, r)) if s.eq_ignore_ascii_case("http") => ("http", r),
        Some((s, r)) if s.eq_ignore_ascii_case("https") => ("https", r),
        Some(_) => return None, // socks5://, ftp://, ...
        None => ("http", entry),
    };
    let rest = rest.trim_end_matches('/');
    // Optional "user:pass@" prefix; the authority is what follows the last '@'.
    let authority = rest.rsplit_once('@').map_or(rest, |(_, a)| a);
    if !valid_host_port(authority) {
        return None;
    }
    Some(format!("{scheme}://{rest}"))
}

fn valid_host_port(authority: &str) -> bool {
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        // [::1] or [::1]:8080
        let Some((h, tail)) = v6.split_once(']') else {
            return false;
        };
        if h.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        match tail {
            "" => (h, None),
            t => match t.strip_prefix(':') {
                Some(p) => (h, Some(p)),
                None => return false,
            },
        }
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || "/?#@\\;=".contains(c)) {
        return false;
    }
    match port {
        None => true,
        Some(p) => {
            p.chars().all(|c| c.is_ascii_digit()) && matches!(p.parse::<u32>(), Ok(1..=65535))
        }
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

#[cfg(test)]
mod tests {
    use super::parse_proxy_server as p;

    #[test]
    fn plain_host_port_gets_http() {
        assert_eq!(p("127.0.0.1:7890").as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(p("  proxy.corp:8080 ").as_deref(), Some("http://proxy.corp:8080"));
    }

    #[test]
    fn existing_scheme_is_not_doubled() {
        // Issue #124 (Hiddify).
        assert_eq!(p("http://127.0.0.1:12334").as_deref(), Some("http://127.0.0.1:12334"));
        assert_eq!(p("HTTP://127.0.0.1:12334").as_deref(), Some("http://127.0.0.1:12334"));
        assert_eq!(p("https://proxy.corp:443").as_deref(), Some("https://proxy.corp:443"));
        assert_eq!(p("http://127.0.0.1:12334/").as_deref(), Some("http://127.0.0.1:12334"));
    }

    #[test]
    fn per_protocol_prefers_https_then_http() {
        assert_eq!(
            p("http=a:1;https=b:2;ftp=c:3").as_deref(),
            Some("http://b:2")
        );
        assert_eq!(p("ftp=c:3;http=a:1").as_deref(), Some("http://a:1"));
        assert_eq!(
            p("http=http://a:1;https=http://b:2").as_deref(),
            Some("http://b:2")
        );
        assert_eq!(p("HTTPS=b:2").as_deref(), Some("http://b:2"));
    }

    #[test]
    fn socks_and_unknown_are_rejected() {
        assert_eq!(p("socks=127.0.0.1:1080"), None);
        assert_eq!(p("socks5://127.0.0.1:1080"), None);
        assert_eq!(p("socks://127.0.0.1:1080"), None);
        assert_eq!(p("ftp=a:1"), None);
        assert_eq!(p("ftp://a:1"), None);
        assert_eq!(p("http=socks5://a:1"), None);
    }

    #[test]
    fn malformed_values_are_rejected() {
        assert_eq!(p(""), None);
        assert_eq!(p("   "), None);
        assert_eq!(p("http://"), None);
        assert_eq!(p("host:port"), None);
        assert_eq!(p("host:"), None);
        assert_eq!(p("a b:80"), None);
        assert_eq!(p("http://http://127.0.0.1:1"), None);
        assert_eq!(p("http=;https="), None);
    }

    #[test]
    fn out_of_range_port_and_bad_ipv6_rejected() {
        assert_eq!(p("127.0.0.1:99999"), None);
        assert_eq!(p("127.0.0.1:0"), None);
        assert_eq!(p("127.0.0.1:65536"), None);
        assert_eq!(p("127.0.0.1:65535").as_deref(), Some("http://127.0.0.1:65535"));
        assert_eq!(p("[not-ip]:8080"), None);
        assert_eq!(p("[::1"), None);
    }

    #[test]
    fn empty_https_entry_falls_back_to_http() {
        assert_eq!(
            p("http=127.0.0.1:7890;https= ;").as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            p("http=127.0.0.1:7890;https=bad:port").as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn list_whitespace_and_trailing_semicolon() {
        assert_eq!(
            p(" http=a:1 ; https=b:2 ; ").as_deref(),
            Some("http://b:2")
        );
        assert_eq!(p("http=a:1;").as_deref(), Some("http://a:1"));
        assert_eq!(p(";;").as_deref(), None);
    }

    #[test]
    fn credentials_without_scheme() {
        assert_eq!(
            p("user:pass@proxy.corp:8080").as_deref(),
            Some("http://user:pass@proxy.corp:8080")
        );
        assert_eq!(
            p("user:p=ss@proxy.corp:8080").as_deref(),
            Some("http://user:p=ss@proxy.corp:8080")
        );
    }

    #[test]
    fn ipv6_and_credentials() {
        assert_eq!(p("[::1]:8080").as_deref(), Some("http://[::1]:8080"));
        assert_eq!(p("http://[::1]:8080").as_deref(), Some("http://[::1]:8080"));
        assert_eq!(
            p("http://user:pw@proxy.corp:8080").as_deref(),
            Some("http://user:pw@proxy.corp:8080")
        );
        assert_eq!(p("http://user:pw@:8080"), None);
    }
}
