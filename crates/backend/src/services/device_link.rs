//! Device-link flow — port of PopoverView.runDeviceFlow / SettingsView.relink.
//! The command returns the user code immediately; completion is pushed via
//! the `device-link` event.

use crate::services::api_client::{poll_device_code, request_device_code};
use crate::state::AppCtx;
use crate::Backend;
use serde::Serialize;
use std::time::Duration;
use vibe_core::config::{resolve_api_url, verification_url, VibeUsageConfig};

pub const CLIENT_NAME: &str = "Vibe Usage for Windows";

#[derive(Default)]
pub(crate) struct Attempt {
    generation: u64,
    poll: Option<tokio::task::AbortHandle>,
}

impl Attempt {
    fn restart(&mut self) -> u64 {
        if let Some(task) = self.poll.take() {
            task.abort();
        }
        self.generation += 1;
        self.generation
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum DeviceLink<'a> {
    Success,
    Error { message: &'a str },
    Denied,
    Expired,
}

fn emit(app: &Backend, payload: DeviceLink<'_>) {
    let _ = app.emit("device-link", payload);
}

/// Starts the flow: requests a code, opens the browser, spawns the poll loop.
/// Returns the user code for on-screen verification.
pub async fn start(app: Backend) -> Result<String, String> {
    start_with(app, |http, url| async move {
        request_device_code(&http, &url, CLIENT_NAME, AppCtx::hostname().as_deref())
            .await
            .map_err(|e| e.to_string())
    })
    .await
}

async fn start_with<F, Fut>(app: Backend, code: F) -> Result<String, String>
where
    F: FnOnce(reqwest::Client, String) -> Fut,
    Fut: std::future::Future<Output = Result<super::api_client::DeviceCodeResponse, String>>,
{
    let id = app.ctx().device_link.lock().unwrap().restart();
    app.ctx().wait_launch_network().await;
    if app.ctx().device_link.lock().unwrap().generation != id {
        return Err("已取消".into());
    }

    let (http, base_url) = {
        let ctx = app.ctx();
        (ctx.http.clone(), ctx.config.load_api_url())
    };

    let response = code(http, base_url.clone()).await;
    let mut attempt = app.ctx().device_link.lock().unwrap();
    if attempt.generation != id {
        return Err("已取消".into());
    }
    let device = response?;

    // System default browser. On Windows this goes through an isolated helper
    // process so shell/browser handler crashes cannot take down the tray app.
    let approval_url = verification_url(&base_url, &device.verification_uri_complete);
    if let Err(e) = crate::process_utils::shell_open(&approval_url) {
        log::error!("open browser failed: {e}");
    }

    let user_code = device.user_code.clone();
    let app2 = app.clone();
    attempt.poll = app.spawn(async move {
        poll_loop(
            app2,
            id,
            base_url,
            device.device_code,
            device.interval,
            device.expires_in,
        )
        .await;
    });
    Ok(user_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn success_matches_the_host_fixture() {
        let sample: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../native/contract/events/device-link.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(DeviceLink::Success).unwrap(),
            sample["payload"]
        );
        let failure = DeviceLink::Error { message: "fixture" };
        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            serde_json::json!({"status": "error", "message": "fixture"})
        );
    }

    #[tokio::test]
    async fn a_late_code_cannot_reopen_the_browser_or_replace_the_new_poll() {
        struct Host;
        impl crate::Host for Host {
            fn event(&self, _: &str, _: serde_json::Value) {}
            fn request_exit(&self) {}
        }
        let dir = tempfile::tempdir().unwrap();
        let app = Backend::new(
            crate::Options {
                roaming_config: dir.path().into(),
                local_data: dir.path().into(),
                resources: dir.path().into(),
                version: "1".into(),
                autostart: false,
            },
            Arc::new(Host),
            vibe_core::config::ConfigManager::with_dir(dir.path().into(), false),
            Arc::new(crate::workflow::Live),
        );
        let (send, pending) = tokio::sync::oneshot::channel();
        let first = start_with(app.clone(), |_, _| async { pending.await.unwrap() });
        tokio::pin!(first);
        tokio::select! { biased; _ = &mut first => panic!("waiting for code"), _ = std::future::ready(()) => {} }
        cancel(&app);
        let second = start_with(app.clone(), |_, _| std::future::pending());
        tokio::pin!(second);
        tokio::select! { biased; _ = &mut second => panic!("waiting for code"), _ = std::future::ready(()) => {} }
        send.send(Ok(super::super::api_client::DeviceCodeResponse {
            device_code: "old".into(),
            user_code: "old".into(),
            verification_uri: None,
            verification_uri_complete: "invalid:old".into(),
            expires_in: 60,
            interval: 1,
        }))
        .unwrap();
        assert_eq!(first.await.unwrap_err(), "已取消");
        assert!(app.ctx().device_link.lock().unwrap().poll.is_none());
    }
}

pub fn cancel(app: &Backend) {
    app.ctx().device_link.lock().unwrap().restart();
}

async fn poll_loop(
    app: Backend,
    id: u64,
    base_url: String,
    device_code: String,
    interval: u64,
    expires_in: u64,
) {
    let http = {
        let ctx = app.ctx();
        ctx.http.clone()
    };
    let interval = Duration::from_secs(interval.max(1));
    let deadline = std::time::Instant::now() + Duration::from_secs(expires_in.max(1));

    while std::time::Instant::now() < deadline {
        tokio::time::sleep(interval).await;

        let res = match poll_device_code(&http, &base_url, &device_code).await {
            Ok(r) => r,
            // Transient network errors keep polling (mirrors `catch { continue }`).
            Err(_) => continue,
        };

        let attempt = app.ctx().device_link.lock().unwrap();
        if attempt.generation != id {
            return;
        }
        if let Some(api_key) = res.api_key {
            let api_url = resolve_api_url(res.api_url.as_deref(), &base_url);
            match configure(&app, api_key, api_url.to_owned()) {
                Ok(()) => emit(&app, DeviceLink::Success),
                Err(_) => emit(
                    &app,
                    DeviceLink::Error {
                        message: "无法保存配置",
                    },
                ),
            }
            return;
        }
        match res.error.as_deref() {
            Some("authorization_pending") | None => continue,
            Some("access_denied") => {
                emit(&app, DeviceLink::Denied);
                return;
            }
            Some("expired_token") => {
                emit(&app, DeviceLink::Expired);
                return;
            }
            Some(other) => {
                emit(&app, DeviceLink::Error { message: other });
                return;
            }
        }
    }
    if app.ctx().device_link.lock().unwrap().generation == id {
        emit(&app, DeviceLink::Expired);
    }
}

/// Save config + start the scheduler — port of AppState.configure.
/// Hostname is stamped once at link time (the CLI reuses it on every sync).
fn configure(app: &Backend, api_key: String, api_url: String) -> Result<(), String> {
    let ctx = app.ctx();
    let mut generation = ctx.account_generation.lock().unwrap();
    let mut cfg = ctx.config.load().unwrap_or_else(VibeUsageConfig::default);
    cfg.api_key = Some(api_key);
    cfg.api_url = Some(api_url);
    if cfg.hostname.is_none() {
        cfg.hostname = AppCtx::hostname();
    }
    ctx.config.save(&cfg).map_err(|e| e.to_string())?;
    *generation += 1;
    app.reset_sync_state()?;
    crate::services::scheduler::start(app.clone());
    Ok(())
}
