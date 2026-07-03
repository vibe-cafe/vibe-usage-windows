//! Subscription quota (订阅配额) types + readers — port of Models/RateLimit.swift,
//! Services/CodexRateLimitReader.swift and Services/ClaudeRateLimitReader.swift.

pub mod claude;
pub mod codex;

use serde::Serialize;

/// One subscription window (5h or 7d). Serialized camelCase for the frontend.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    /// 0-100
    pub utilization: f64,
    /// epoch seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<f64>,
    /// seconds; present → the elapsed-time bar can render
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_duration: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RateLimitProvider {
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "claudeCode")]
    ClaudeCode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RateLimitStatus {
    Ok,
    NoData,
    Disabled,
    Unauthorized,
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRateLimit {
    pub provider: RateLimitProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<RateLimitWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<RateLimitWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_label: Option<String>,
    pub status: RateLimitStatus,
}

impl ProviderRateLimit {
    pub fn empty(provider: RateLimitProvider, status: RateLimitStatus) -> Self {
        Self {
            provider,
            five_hour: None,
            seven_day: None,
            plan_label: None,
            status,
        }
    }
}

pub(crate) fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
