//! Vibe Usage HTTP client — port of Services/APIClient.swift.

use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use vibe_core::config::ConfigManager;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UsageQuery {
    Days { days: u32 },
    From { from_iso: String },
    Custom { from_date: String, to_date: String },
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Http(u16),
    Network(String),
    Decode(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "API Key 无效"),
            ApiError::Http(code) => write!(f, "HTTP 错误 {code}"),
            ApiError::Network(m) => write!(f, "网络错误：{m}"),
            ApiError::Decode(m) => write!(f, "服务器响应异常：{m}"),
        }
    }
}

/// The panel and its tray totals read usage for the currently linked account.
pub async fn fetch_linked_usage(
    http: &reqwest::Client,
    config: &ConfigManager,
    query: &UsageQuery,
    launch_network: impl std::future::Future<Output = ()>,
) -> Result<Value, String> {
    // A reset or relink can change both the key and its server during the wait.
    launch_network.await;
    let linked = config.load().ok_or("未配置")?;
    let base_url = config.effective_api_url(&linked);
    let api_key = linked.api_key.ok_or("未配置")?;
    fetch_usage(http, &base_url, &api_key, query)
        .await
        .map_err(|e| e.to_string())
}

/// GET /api/usage?…&tz=<IANA tz>. Response passed through to the frontend
/// verbatim (buckets/sessions/hasAnyData).
pub async fn fetch_usage(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    query: &UsageQuery,
) -> Result<Value, ApiError> {
    let mut params: Vec<(String, String)> = match query {
        UsageQuery::Days { days } => vec![("days".into(), days.to_string())],
        UsageQuery::From { from_iso } => vec![("from".into(), from_iso.clone())],
        UsageQuery::Custom {
            from_date,
            to_date,
        } => vec![
            ("from".into(), from_date.clone()),
            ("to".into(), to_date.clone()),
        ],
    };
    // Windows tz names must be converted to IANA ids ("Asia/Shanghai", not
    // "China Standard Time") — iana-time-zone handles the mapping.
    let tz = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
    params.push(("tz".into(), tz));

    let res = http
        .get(format!("{}/api/usage", base_url.trim_end_matches('/')))
        .query(&params)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;

    match res.status().as_u16() {
        200 => res
            .json::<Value>()
            .await
            .map_err(|e| ApiError::Decode(e.to_string())),
        401 => Err(ApiError::Unauthorized),
        code => Err(ApiError::Http(code)),
    }
}

// ---------------------------------------------------------------------------
// Device authorization flow (unauthenticated)

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    #[allow(dead_code)]
    pub verification_uri: Option<String>,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePollResponse {
    pub api_key: Option<String>,
    pub api_url: Option<String>,
    pub error: Option<String>,
}

pub async fn request_device_code(
    http: &reqwest::Client,
    base_url: &str,
    client_name: &str,
    hostname: Option<&str>,
) -> Result<DeviceCodeResponse, ApiError> {
    let mut body = serde_json::json!({ "clientName": client_name });
    if let Some(h) = hostname {
        body["hostname"] = Value::String(h.to_string());
    }
    let res = http
        .post(format!("{}/api/usage/device/code", base_url.trim_end_matches('/')))
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    if res.status().as_u16() != 200 {
        return Err(ApiError::Http(res.status().as_u16()));
    }
    res.json().await.map_err(|e| ApiError::Decode(e.to_string()))
}

/// 200 covers success + pending/denied/expired (RFC 8628 style);
/// 410 is "already delivered, never replay" — still decoded.
pub async fn poll_device_code(
    http: &reqwest::Client,
    base_url: &str,
    device_code: &str,
) -> Result<DevicePollResponse, ApiError> {
    let res = http
        .post(format!("{}/api/usage/device/poll", base_url.trim_end_matches('/')))
        .json(&serde_json::json!({ "deviceCode": device_code }))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    if status == 200 || status == 410 {
        return res.json().await.map_err(|e| ApiError::Decode(e.to_string()));
    }
    Err(ApiError::Http(status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use vibe_core::config::VibeUsageConfig;

    fn link(config: &ConfigManager, server: &TcpListener, account: &str) {
        config
            .save(&VibeUsageConfig {
                api_url: Some(format!("http://{}/{account}", server.local_addr().unwrap())),
                api_key: Some(format!("vbu_{account}")),
                ..Default::default()
            })
            .unwrap();
    }

    #[tokio::test]
    async fn unlink_during_launch_wait_sends_no_usage_request() {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigManager::with_dir(dir.path().into(), false);
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        link(&config, &server, "old");
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let (entered, at_wait) = tokio::sync::oneshot::channel();
        let (release, waiting) = tokio::sync::oneshot::channel();
        let read = fetch_linked_usage(&http, &config, &UsageQuery::Days { days: 1 }, async {
            entered.send(()).unwrap();
            waiting.await.unwrap();
        });
        tokio::pin!(read);
        tokio::select! {
            _ = &mut read => panic!("usage finished before the launch wait"),
            _ = server.accept() => panic!("usage connected before the launch wait"),
            reached = at_wait => reached.expect("usage should reach the launch wait"),
        }
        config.reset().unwrap();
        release.send(()).unwrap();
        tokio::select! {
            result = read => assert_eq!(result.unwrap_err(), "未配置"),
            _ = server.accept() => panic!("the unlinked account sent a request"),
        }
    }

    #[tokio::test]
    async fn relink_during_launch_wait_uses_the_current_server_and_key() {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigManager::with_dir(dir.path().into(), false);
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        link(&config, &server, "old");
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let read = fetch_linked_usage(&http, &config, &UsageQuery::Days { days: 7 }, async {
            link(&config, &server, "current");
        });
        let serve = async {
            let (socket, _) = server.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut headers = String::new();
            while !headers.ends_with("\r\n\r\n") {
                assert_ne!(socket.read_line(&mut headers).await.unwrap(), 0);
            }
            assert!(headers.starts_with("GET /current/api/usage?days=7&"));
            assert!(headers
                .to_ascii_lowercase()
                .contains("\r\nauthorization: bearer vbu_current\r\n"));
            socket
                .get_mut()
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .await
                .unwrap();
        };
        let (result, ()) =
            tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(read, serve) })
                .await
                .expect("the linked usage request should finish");
        assert_eq!(result.unwrap(), serde_json::json!({}));
    }
}
