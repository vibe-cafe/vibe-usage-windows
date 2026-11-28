//! Vibe Usage HTTP client — port of Services/APIClient.swift.

use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UsageQuery {
    Days { days: u32 },
    From { from_iso: String },
    Custom { from_iso: String, to_iso: String, tz: String },
}

impl UsageQuery {
    fn params(&self) -> Vec<(&'static str, String)> {
        let local_zone = || iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
        match self {
            Self::Days { days } => vec![("days", days.to_string()), ("tz", local_zone())],
            Self::From { from_iso } => vec![("from", from_iso.clone()), ("tz", local_zone())],
            // The WebView calculated both instants in this IANA time zone.
            Self::Custom { from_iso, to_iso, tz } => vec![
                ("from", from_iso.clone()),
                ("to", to_iso.clone()),
                ("tz", tz.clone()),
            ],
        }
    }
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

/// GET /api/usage?…&tz=<IANA tz>. Response passed through to the frontend
/// verbatim (buckets/sessions/hasAnyData).
pub async fn fetch_usage(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    query: &UsageQuery,
) -> Result<Value, ApiError> {
    let res = http
        .get(format!("{}/api/usage", base_url.trim_end_matches('/')))
        .query(&query.params())
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

    #[test]
    fn custom_query_passes_the_webview_bounds_and_timezone_to_usage() {
        let query: UsageQuery = serde_json::from_value(serde_json::json!({
            "kind": "custom",
            "fromIso": "2026-11-02T16:00:00.000Z",
            "toIso": "2026-11-18T15:59:59.999Z",
            "tz": "Asia/Shanghai"
        })).unwrap();
        assert_eq!(query.params(), [
            ("from", "2026-11-02T16:00:00.000Z".into()),
            ("to", "2026-11-18T15:59:59.999Z".into()),
            ("tz", "Asia/Shanghai".into()),
        ]);
    }
}
