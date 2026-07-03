//! vibecafe.ai HTTP client — port of Services/APIClient.swift.

use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
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
        .get(format!("{base_url}/api/usage"))
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
        .post(format!("{base_url}/api/usage/device/code"))
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
        .post(format!("{base_url}/api/usage/device/poll"))
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
