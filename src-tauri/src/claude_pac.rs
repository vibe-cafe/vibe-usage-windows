//! Read the first *typed* PAC choice for Claude. The legacy WinHTTP API used
//! by other callers doesn't expose SOCKS in WINHTTP_PROXY_INFO.
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, LazyLock, Mutex,
};
use std::time::{Duration, Instant};
use vibe_core::proxy_route::Route;
use windows_sys::Win32::Foundation::{ERROR_IO_PENDING, ERROR_TIMEOUT};
use windows_sys::Win32::Networking::WinHttp::*;

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}

// WinHTTP callbacks can arrive after cancellation. The context is an ID,
// never a pointer into the worker's stack or a handle that might be reused.
static NEXT: AtomicUsize = AtomicUsize::new(1);
static PENDING: LazyLock<Mutex<HashMap<usize, mpsc::Sender<u32>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

unsafe extern "system" fn completed(
    _: *mut c_void,
    id: usize,
    status: u32,
    info: *mut c_void,
    _: u32,
) {
    let code = match status {
        WINHTTP_CALLBACK_STATUS_GETPROXYFORURL_COMPLETE => 0,
        WINHTTP_CALLBACK_STATUS_REQUEST_ERROR if !info.is_null() => {
            (*(info as *const WINHTTP_ASYNC_RESULT)).dwError
        }
        _ => return,
    };
    if let Some(sender) = PENDING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&id)
    {
        let _ = sender.send(code);
    }
}

pub fn resolve(pac: &str, target: &str, deadline: Instant) -> Option<Route> {
    if Instant::now() >= deadline {
        return None;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let pac = wide(pac);
    let target = wide(target);
    unsafe {
        let raw = WinHttpOpen(
            wide("VibeUsage").as_ptr(),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            null(),
            null(),
            WINHTTP_FLAG_ASYNC,
        );
        if raw.is_null() {
            return None;
        }
        let session = Handle(raw);
        let previous = WinHttpSetStatusCallback(
            session.0,
            Some(completed),
            WINHTTP_CALLBACK_FLAG_GETPROXYFORURL_COMPLETE | WINHTTP_CALLBACK_FLAG_REQUEST_ERROR,
            0,
        );
        if previous.is_some_and(|callback| callback as usize == usize::MAX) {
            return None;
        }
        for login in [0, 1] {
            let mut resolver = null_mut();
            if WinHttpCreateProxyResolver(session.0, &mut resolver) != 0 {
                return None;
            }
            let resolver = Handle(resolver);
            let options = WINHTTP_AUTOPROXY_OPTIONS {
                dwFlags: WINHTTP_AUTOPROXY_CONFIG_URL,
                lpszAutoConfigUrl: pac.as_ptr(),
                fAutoLogonIfChallenged: login,
                ..std::mem::zeroed()
            };
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = mpsc::channel();
            PENDING.lock().unwrap().insert(id, tx);
            let mut code = WinHttpGetProxyForUrlEx(resolver.0, target.as_ptr(), &options, id);
            if code == ERROR_IO_PENDING {
                let timeout = deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(3));
                code = rx.recv_timeout(timeout).unwrap_or(ERROR_TIMEOUT);
            }
            PENDING.lock().unwrap().remove(&id);
            if code == ERROR_WINHTTP_LOGIN_FAILURE && login == 0 && Instant::now() < deadline {
                continue;
            }
            if code != 0 {
                return None;
            }
            let mut result: WINHTTP_PROXY_RESULT = std::mem::zeroed();
            if WinHttpGetProxyResult(resolver.0, &mut result) != 0 {
                return None;
            }
            let route = if result.cEntries > 0 && !result.pEntries.is_null() {
                first_route(&*result.pEntries)
            } else {
                None
            };
            WinHttpFreeProxyResult(&mut result);
            return route;
        }
    }
    None
}

unsafe fn first_route(entry: &WINHTTP_PROXY_RESULT_ENTRY) -> Option<Route> {
    if entry.fProxy == 0 {
        return Some(Route::Direct);
    }
    if entry.pwszProxy.is_null() || entry.ProxyPort == 0 {
        return None;
    }
    let mut length = 0;
    while *entry.pwszProxy.add(length) != 0 {
        length += 1;
    }
    let host = String::from_utf16(std::slice::from_raw_parts(entry.pwszProxy, length)).ok()?;
    if host.is_empty() {
        return None;
    }
    let port = entry.ProxyPort;
    let endpoint = if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    match entry.ProxyScheme {
        WINHTTP_INTERNET_SCHEME_SOCKS => Some(Route::Socks(endpoint)),
        WINHTTP_INTERNET_SCHEME_HTTP => Some(Route::Http(format!("http://{endpoint}"))),
        WINHTTP_INTERNET_SCHEME_HTTPS => Some(Route::Http(format!("https://{endpoint}"))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_winhttp_entries_distinguish_socks_direct_and_unknown_schemes() {
        let mut host: Vec<_> = "127.0.0.1".encode_utf16().chain(Some(0)).collect();
        let mut entry = WINHTTP_PROXY_RESULT_ENTRY {
            fProxy: 1,
            fBypass: 0,
            ProxyScheme: WINHTTP_INTERNET_SCHEME_SOCKS,
            pwszProxy: host.as_mut_ptr(),
            ProxyPort: 1080,
        };
        unsafe {
            assert_eq!(
                first_route(&entry),
                Some(Route::Socks("127.0.0.1:1080".into()))
            );
            entry.ProxyScheme = WINHTTP_INTERNET_SCHEME_HTTPS;
            assert_eq!(
                first_route(&entry),
                Some(Route::Http("https://127.0.0.1:1080".into()))
            );
            entry.ProxyScheme = WINHTTP_INTERNET_SCHEME_FTP;
            assert_eq!(first_route(&entry), None);
            entry.fProxy = 0;
            assert_eq!(first_route(&entry), Some(Route::Direct));
        }
    }
}
