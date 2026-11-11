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

/// Command for a trusted executable path we created ourselves, such as the
/// verified updater installer. No shell is involved: each argument reaches the
/// program as its own token, and the path is quoted for us.
pub fn executable_command(path: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(path);
    cmd.args(args);
    hide_command_window(&mut cmd);
    cmd
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
/// than a malformed URL. Final validation uses the WHATWG `url` parser, the
/// same rules Node applies, so anything accepted here Node can also parse.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_proxy_server(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Per-protocol list only when every ';'-separated item is "<known protocol>=...".
    // A '=' elsewhere (e.g. user "alice=name:pw@host") does not make a list.
    const PROTOCOLS: [&str; 5] = ["http", "https", "ftp", "socks", "gopher"];
    let parts: Vec<&str> = raw
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let items: Vec<(&str, &str)> = parts.iter().filter_map(|p| p.split_once('=')).collect();
    let is_list = !parts.is_empty()
        && items.len() == parts.len()
        && items
            .iter()
            .all(|(k, _)| PROTOCOLS.iter().any(|n| k.trim().eq_ignore_ascii_case(n)));
    if !is_list {
        return normalize_proxy_entry(raw);
    }
    // Try https first, then http; an empty/invalid https entry falls back.
    ["https", "http"].into_iter().find_map(|name| {
        items
            .iter()
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
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(&format!("{scheme}://{rest}")).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return None;
    }
    // Node later runs decodeURIComponent() on the credentials; reject values
    // it would throw on (malformed %XX or non-UTF-8 bytes).
    if !decodes_as_utf8(url.username()) || !decodes_as_utf8(url.password().unwrap_or("")) {
        return None;
    }
    match url.host()? {
        url::Host::Domain(d) => {
            // Reject leftovers the URL parser tolerates but no real host has
            // (e.g. ';;'). IDNs are already Punycode (ASCII) here.
            if d.is_empty()
                || !d
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                return None;
            }
        }
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => {}
    }
    Some(url.as_str().trim_end_matches('/').to_string())
}

/// True when percent-decoding `s` succeeds and yields valid UTF-8.
fn decodes_as_utf8(s: &str) -> bool {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            // from_str_radix would accept "+1"; require two real hex digits.
            let hex = b.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit));
            match hex.and_then(|h| std::str::from_utf8(h).ok()) {
                Some(h) => out.push(u8::from_str_radix(h, 16).unwrap_or(0)),
                None => return false,
            }
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).is_ok()
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
        assert_eq!(p("https://proxy.corp:443").as_deref(), Some("https://proxy.corp"));
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
            Some("http://user:p%3Dss@proxy.corp:8080")
        );
    }

    #[test]
    fn host_whitelist_and_userinfo_validation() {
        // Invalid https entry must fall back to the valid http one.
        assert_eq!(
            p("http=127.0.0.1:7890;https=bad]:8080").as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(p("bad]:8080"), None);
        assert_eq!(p("ba[d:8080"), None);
        assert_eq!(p("ho%st:8080"), None);
        assert_eq!(p("127.0.0.999:7890"), None);
        assert_eq!(
            p("http=127.0.0.1:7890;https=127.0.0.999:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(p("under_score.host-1:80").as_deref(), Some("http://under_score.host-1"));
        // userinfo
        assert_eq!(p("us er:pw@h:80"), None);
        assert_eq!(p("user:p/w@h:80"), None);
        assert_eq!(p("user:p@w@h:80").as_deref(), Some("http://user:p%40w@h"));
        assert_eq!(
            p("us.er:p%40w!$&'()*+,~@h:80").as_deref(),
            Some("http://us.er:p%40w!$&'()*+,~@h")
        );
        assert_eq!(
            p("http=bad user:x@h:1;https=h:2").as_deref(),
            Some("http://h:2")
        );
    }

    #[test]
    fn idn_hosts_become_punycode() {
        assert_eq!(p("h\u{00e9}st:8080").as_deref(), Some("http://xn--hst-bma:8080"));
        let idn = p("\u{4f8b}\u{5b50}.\u{6d4b}\u{8bd5}:8080").unwrap();
        assert!(idn.starts_with("http://xn--") && idn.ends_with(":8080"), "{idn}");
        assert_eq!(p("intranet_box:3128").as_deref(), Some("http://intranet_box:3128"));
    }

    #[test]
    fn equals_in_username_is_not_a_list() {
        assert_eq!(
            p("alice=name:pw@proxy.corp:8080").as_deref(),
            Some("http://alice%3Dname:pw@proxy.corp:8080")
        );
        // Unknown protocol key in a list-looking value: not a list, not valid.
        assert_eq!(p("foo=a:1"), None);
    }

    #[test]
    fn bad_percent_encoding_in_credentials_falls_back() {
        for bad in ["user:p%w@proxy.corp:8080", "user:p%FF@proxy.corp:8080", "us%@h:80", "u%4@h:80", "u:p%+1@h:80", "u:p%-1@h:80"] {
            assert_eq!(p(bad), None, "{bad}");
            assert_eq!(
                p(&format!("http=127.0.0.1:7890;https={bad}")).as_deref(),
                Some("http://127.0.0.1:7890"),
                "{bad}"
            );
        }
        assert_eq!(
            p("user:p%C3%A9@h:80").as_deref(),
            Some("http://user:p%C3%A9@h")
        );
    }

    #[test]
    fn path_query_fragment_rejected() {
        assert_eq!(p("127.0.0.1:7890/proxy"), None);
        assert_eq!(p("http://127.0.0.1:7890/pac.js"), None);
        assert_eq!(p("127.0.0.1:7890?x=1"), None);
        assert_eq!(p("127.0.0.1:7890#frag"), None);
        assert_eq!(
            p("http=127.0.0.1:7890;https=h:1/path").as_deref(),
            Some("http://127.0.0.1:7890")
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
