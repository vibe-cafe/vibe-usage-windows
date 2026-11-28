//! ZCode API-key storage. Release builds use the current Windows user's
//! encrypted Credential Manager vault; keys never enter settings.json.

use serde::Serialize;
use vibe_core::quota_product::ZCodeQuotaRegion;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZCodeCredentialStatus {
    pub big_model_configured: bool,
    pub z_ai_configured: bool,
}

pub fn status() -> Result<ZCodeCredentialStatus, String> {
    Ok(ZCodeCredentialStatus {
        big_model_configured: load(ZCodeQuotaRegion::BigModel)?.is_some(),
        z_ai_configured: load(ZCodeQuotaRegion::ZAi)?.is_some(),
    })
}

pub fn load(region: ZCodeQuotaRegion) -> Result<Option<String>, String> {
    platform::load(target(region))
}

pub fn store(region: ZCodeQuotaRegion, value: Option<&str>) -> Result<(), String> {
    let trimmed = value.map(str::trim).filter(|value| !value.is_empty());
    match trimmed {
        Some(secret) => platform::store(target(region), secret),
        None => platform::delete(target(region)),
    }
}

fn target(region: ZCodeQuotaRegion) -> &'static str {
    match region {
        ZCodeQuotaRegion::BigModel => "ai.vibecafe.vibe-usage.windows/zcode/bigmodel",
        ZCodeQuotaRegion::ZAi => "ai.vibecafe.vibe-usage.windows/zcode/zai",
    }
}

/// The entry for `target`, through the app's own Credential Manager access
/// (`credential_store`), with the messages this card and the settings show.
mod platform {
    use crate::services::credential_store::{self, StoreError};

    pub fn load(target: &str) -> Result<Option<String>, String> {
        let Some(bytes) = credential_store::load(target).map_err(|_| "无法读取 Windows 凭据管理器".to_string())? else {
            return Ok(None);
        };
        let value = String::from_utf8(bytes)
            .map_err(|_| "Windows 凭据管理器中的 ZCode Key 格式无效".to_string())?;
        let value = value.trim().to_string();
        Ok((!value.is_empty()).then_some(value))
    }

    pub fn store(target: &str, secret: &str) -> Result<(), String> {
        credential_store::store(target, secret.as_bytes()).map_err(|error| match error {
            #[cfg(not(windows))]
            StoreError::Unavailable => "ZCode Key 只能在 Windows 凭据管理器中保存".to_string(),
            StoreError::Failed => "无法写入 Windows 凭据管理器".to_string(),
        })
    }

    pub fn delete(target: &str) -> Result<(), String> {
        credential_store::delete(target).map_err(|_| "无法删除 Windows 凭据管理器中的 ZCode Key".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_use_separate_credential_targets() {
        assert_ne!(
            target(ZCodeQuotaRegion::BigModel),
            target(ZCodeQuotaRegion::ZAi)
        );
        assert_eq!(
            ZCodeQuotaRegion::BigModel.environment_key(),
            "BIGMODEL_API_KEY"
        );
        assert_eq!(ZCodeQuotaRegion::ZAi.environment_key(), "Z_AI_API_KEY");
    }
}
