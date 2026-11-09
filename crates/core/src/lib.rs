//! Platform-agnostic core logic for Vibe Usage for Windows.
//!
//! Everything here is unit-testable on any OS (paths are injectable). The
//! Tauri shell (`src-tauri`) wires these pieces to windows/tray/network.

pub mod claude_code;
pub mod config;
pub mod quota_product;
pub mod rate_limit;
pub mod runtime;
pub mod statusline_hook;
pub mod tray_text;
pub mod version;

pub use rate_limit::{
    ProviderRateLimit, RateLimitEmptyReason, RateLimitLiveFailure, RateLimitMeter,
    RateLimitProvider, RateLimitSource, RateLimitStatus, RateLimitWindow,
};
