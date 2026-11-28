//! Typed stand-ins for command results with private fields or nested types.
use super::*;

pub fn app_status() -> AppStatus {
    AppStatus {
        configured: false,
        hostname: Some("fixture".into()),
        api_url: "https://example.invalid".into(),
        version: "0.8.0".into(),
        is_dev: false,
        runtime_available: false,
        test_diagnostics_available: false,
        updates_available: true,
        api_key_display: None,
    }
}

pub fn device_code() -> DeviceLinkStart {
    DeviceLinkStart {
        user_code: "SAMPLE".into(),
    }
}

pub fn products() -> Vec<QuotaProduct> {
    vec![QuotaProduct {
        provider: RateLimitProvider::KimiCode,
        display_name: "Kimi Code",
        availability: quota_product::QuotaProductAvailability::Ready,
        is_detected: true,
    }]
}

pub fn credentials() -> zcode_credentials::ZCodeCredentialStatus {
    zcode_credentials::ZCodeCredentialStatus {
        big_model_configured: false,
        z_ai_configured: true,
    }
}

pub fn login(waiting: bool) -> cursor_login::LoginStatus {
    use cursor_login::LoginState;
    cursor_login::LoginStatus {
        state: if waiting {
            LoginState::Waiting
        } else {
            LoginState::SignedOut
        },
        email: None,
        supported: waiting.then_some(true),
        error: None,
    }
}
