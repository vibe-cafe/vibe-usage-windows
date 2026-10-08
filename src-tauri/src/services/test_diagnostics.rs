//! Local, exportable diagnostics for debug and explicitly flagged external
//! test builds. Ordinary release builds retain only inert stubs.

use std::path::Path;
use vibe_core::{ProviderRateLimit, RateLimitProvider};

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
use vibe_core::RateLimitStatus;

pub const fn available() -> bool {
    cfg!(any(debug_assertions, feature = "external-test-diagnostics"))
}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
mod enabled {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    const CURRENT: &str = "diagnostics.jsonl";
    const PREVIOUS: &str = "diagnostics.previous.jsonl";
    const MAXIMUM_SIZE: u64 = 1_000_000;

    static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        timestamp: String,
        event: &'static str,
        providers: Vec<RateLimitProvider>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        meter_count: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_code: Option<&'static str>,
        app_version: &'static str,
        app_build: &'static str,
        build_kind: &'static str,
        app_commit: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        cli_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cli_commit: Option<String>,
        os: &'static str,
        architecture: &'static str,
    }

    /// The snapshot's recorded provenance. A snapshot vendored from a registry
    /// release has no reviewed commit, so both fields stay optional and unknown
    /// values are omitted from the entry rather than reported as empty strings.
    #[derive(Default, Deserialize)]
    struct CliSource {
        version: Option<String>,
        commit: Option<String>,
    }

    pub(super) fn record_discovery(providers: Vec<RateLimitProvider>) {
        record("quota_products_discovered", providers, None, None, None);
    }

    pub(super) fn record_selection(event: &'static str, providers: Vec<RateLimitProvider>) {
        record(event, providers, None, None, None);
    }

    pub(super) fn record_refresh_started(providers: Vec<RateLimitProvider>) {
        record("quota_refresh_started", providers, None, None, None);
    }

    pub(super) fn record_result(snapshot: &ProviderRateLimit) {
        let typed_windows = [snapshot.five_hour.as_ref(), snapshot.seven_day.as_ref()]
            .into_iter()
            .flatten()
            .count();
        record(
            "quota_refresh_result",
            vec![snapshot.provider],
            Some(status_code(&snapshot.status)),
            Some(snapshot.meters.len() + typed_windows),
            None,
        );
    }

    pub(super) fn record_failure(providers: Vec<RateLimitProvider>, error_code: &'static str) {
        record(
            "quota_refresh_failed",
            providers,
            None,
            None,
            Some(error_code),
        );
    }

    pub(super) fn export(destination: &Path) -> Result<(), String> {
        record("diagnostics_exported", Vec::new(), None, None, None);
        let _guard = WRITE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = diagnostics_directory();
        let mut combined = Vec::new();
        for filename in [PREVIOUS, CURRENT] {
            if let Ok(data) = fs::read(directory.join(filename)) {
                combined.extend_from_slice(&data);
            }
        }
        fs::write(destination, combined).map_err(|_| "无法导出测试诊断日志".to_string())
    }

    fn record(
        event: &'static str,
        providers: Vec<RateLimitProvider>,
        status: Option<&'static str>,
        meter_count: Option<usize>,
        error_code: Option<&'static str>,
    ) {
        let source = cli_source();
        let entry = Entry {
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            event,
            providers,
            status,
            meter_count,
            error_code,
            app_version: env!("CARGO_PKG_VERSION"),
            app_build: option_env!("VIBE_USAGE_APP_BUILD").unwrap_or("unknown"),
            build_kind: option_env!("VIBE_USAGE_BUILD_KIND").unwrap_or(
                if cfg!(feature = "external-test-diagnostics") {
                    "external-test"
                } else {
                    "debug"
                },
            ),
            app_commit: option_env!("VIBE_USAGE_APP_COMMIT").unwrap_or("unknown"),
            cli_version: source.version,
            cli_commit: source.commit,
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        };
        append(&diagnostics_directory(), &entry);
    }

    fn append(directory: &Path, entry: &Entry) {
        let _guard = WRITE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if fs::create_dir_all(directory).is_err() {
            return;
        }
        let current = directory.join(CURRENT);
        if fs::metadata(&current).is_ok_and(|metadata| metadata.len() >= MAXIMUM_SIZE) {
            let previous = directory.join(PREVIOUS);
            let _ = fs::remove_file(&previous);
            let _ = fs::rename(&current, previous);
        }
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(current) else {
            return;
        };
        let Ok(mut data) = serde_json::to_vec(entry) else {
            return;
        };
        data.push(b'\n');
        let _ = file.write_all(&data);
    }

    fn diagnostics_directory() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Vibe Usage")
            .join("Test Diagnostics")
    }

    fn cli_source() -> CliSource {
        // The CLI is no longer bundled: the launcher resolves `latest` at run
        // time and caches the version it got, so a build cannot state one at
        // compile time. Report the channel; the resolved version lives in the
        // launcher's cache under the app's data dir.
        CliSource { version: Some("latest".to_string()), commit: None }
    }

    fn status_code(status: &RateLimitStatus) -> &'static str {
        match status {
            RateLimitStatus::Ok => "ok",
            RateLimitStatus::NoData => "no_data",
            RateLimitStatus::Disabled => "disabled",
            RateLimitStatus::Unauthorized => "unauthorized",
            RateLimitStatus::RetryableError => "retryable_error",
            RateLimitStatus::Error { .. } => "error",
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn serialized_schema_has_no_free_form_or_secret_fields() {
            let source = cli_source();
            let entry = Entry {
                timestamp: "2026-09-08T00:00:00.000Z".into(),
                event: "quota_refresh_failed",
                providers: vec![RateLimitProvider::ZCode],
                status: None,
                meter_count: None,
                error_code: Some("cli_process_failure"),
                app_version: "0.0.0",
                app_build: "1",
                build_kind: "debug",
                app_commit: "abc",
                cli_version: source.version,
                cli_commit: source.commit,
                os: "windows",
                architecture: "x86_64",
            };
            let value = serde_json::to_value(entry).unwrap();
            let object = value.as_object().unwrap();
            for forbidden in [
                "message", "path", "account", "apiKey", "token", "cookie", "stderr", "body",
            ] {
                assert!(!object.contains_key(forbidden));
            }
        }
    }
}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
pub fn record_discovery(providers: Vec<RateLimitProvider>) {
    enabled::record_discovery(providers);
}

#[cfg(not(any(debug_assertions, feature = "external-test-diagnostics")))]
pub fn record_discovery(_providers: Vec<RateLimitProvider>) {}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
pub fn record_selection_initialized(providers: Vec<RateLimitProvider>) {
    enabled::record_selection("quota_selection_initialized", providers);
}

#[cfg(not(any(debug_assertions, feature = "external-test-diagnostics")))]
pub fn record_selection_initialized(_providers: Vec<RateLimitProvider>) {}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
pub fn record_selection_changed(providers: Vec<RateLimitProvider>) {
    enabled::record_selection("quota_selection_changed", providers);
}

#[cfg(not(any(debug_assertions, feature = "external-test-diagnostics")))]
pub fn record_selection_changed(_providers: Vec<RateLimitProvider>) {}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
pub fn record_refresh_started(providers: Vec<RateLimitProvider>) {
    enabled::record_refresh_started(providers);
}

#[cfg(not(any(debug_assertions, feature = "external-test-diagnostics")))]
pub fn record_refresh_started(_providers: Vec<RateLimitProvider>) {}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
pub fn record_result(snapshot: &ProviderRateLimit) {
    enabled::record_result(snapshot);
}

#[cfg(not(any(debug_assertions, feature = "external-test-diagnostics")))]
pub fn record_result(_snapshot: &ProviderRateLimit) {}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
pub fn record_failure(providers: Vec<RateLimitProvider>, error_code: &'static str) {
    enabled::record_failure(providers, error_code);
}

#[cfg(not(any(debug_assertions, feature = "external-test-diagnostics")))]
pub fn record_failure(_providers: Vec<RateLimitProvider>, _error_code: &'static str) {}

#[cfg(any(debug_assertions, feature = "external-test-diagnostics"))]
pub fn export(destination: &Path) -> Result<(), String> {
    enabled::export(destination)
}

#[cfg(not(any(debug_assertions, feature = "external-test-diagnostics")))]
pub fn export(_destination: &Path) -> Result<(), String> {
    Err("正式版不提供测试诊断日志".into())
}
