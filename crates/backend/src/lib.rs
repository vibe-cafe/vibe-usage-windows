//! Service layer for the native Windows app. Call from the host's Tokio runtime.
pub mod commands;
mod egress_check;
mod process_lifecycle;
mod process_utils;
mod services;
mod state;
mod system_proxy;
mod workflow;

pub use egress_check::EgressMode;
use serde::Serialize;
use serde_json::Value;
pub use services::api_client::UsageQuery;
pub use services::auto_launch::launched_at_login;
pub use services::cursor_login::LoginStatus;
pub use services::rate_limits::CachedRateLimits;
pub use services::sync_history::SyncRecord;
pub use services::zcode_credentials::ZCodeCredentialStatus;
pub use state::{AppSettings, SyncState, SyncStatus, UpdateInfo};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::task::{AbortHandle, JoinSet};
use vibe_core::config::ConfigManager;
pub use workflow::UsageSnapshot;

pub struct Options {
    pub roaming_config: PathBuf,
    pub local_data: PathBuf,
    pub resources: PathBuf,
    pub version: String,
    pub autostart: bool,
}

pub trait Host: Send + Sync {
    /// Post to the UI thread. Callbacks may arrive before initialize returns.
    fn event(&self, name: &str, payload: Value);
    fn request_exit(&self);
}

#[derive(Clone)]
pub struct Backend(Arc<Inner>);

struct Inner {
    options: Options,
    host: Arc<dyn Host>,
    events: tokio::sync::mpsc::UnboundedSender<(&'static str, Value)>,
    ctx: state::AppCtx,
    network: Arc<dyn workflow::Network>,
    tasks: Mutex<Option<JoinSet<()>>>,
    usage: workflow::Usage,
    shutdown: tokio::sync::OnceCell<bool>,
}

impl Backend {
    pub fn initialize(options: Options, host: Arc<dyn Host>) -> Self {
        Self::initialize_with(
            options,
            host,
            ConfigManager::new(state::IS_DEV),
            Arc::new(workflow::Live),
            |app| {
                commands::initialize_quota_selection(app.ctx(), &app.options().local_data);
                services::auto_launch::migrate_run_value();
                services::claude_usage::set_bundled_node(services::sync_engine::bundled_node(app));
                let _ = services::rate_limits::statusline_hook(app).retire_legacy();
            },
        )
    }

    fn initialize_with(
        options: Options,
        host: Arc<dyn Host>,
        config: ConfigManager,
        network: Arc<dyn workflow::Network>,
        prepare_local: impl FnOnce(&Backend),
    ) -> Self {
        let app = Self::new(options, host, config, network);
        prepare_local(&app);
        app.start_network();
        app
    }

    fn new(
        options: Options,
        host: Arc<dyn Host>,
        config: ConfigManager,
        network: Arc<dyn workflow::Network>,
    ) -> Self {
        let ctx = state::AppCtx::with_config(
            options.roaming_config.clone(),
            config,
            !options.autostart,
            &options.version,
        );
        let (events, mut pending) = tokio::sync::mpsc::unbounded_channel();
        let app = Self(Arc::new(Inner {
            options,
            host: host.clone(),
            events,
            ctx,
            network,
            usage: Default::default(),
            tasks: Mutex::new(Some(JoinSet::new())),
            shutdown: Default::default(),
        }));
        // Never enter host code under a service's state/credentials lock.
        app.spawn(async move {
            while let Some((name, payload)) = pending.recv().await {
                host.event(name, payload);
            }
        });
        app
    }

    fn options(&self) -> &Options {
        &self.0.options
    }
    fn ctx(&self) -> &state::AppCtx {
        &self.0.ctx
    }

    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) -> Option<AbortHandle> {
        let mut tasks = self.0.tasks.lock().unwrap();
        let tasks = tasks.as_mut()?;
        while tasks.try_join_next().is_some() {}
        Some(tasks.spawn(future))
    }

    fn emit(&self, name: &'static str, payload: impl Serialize) -> Result<(), serde_json::Error> {
        let _ = self.0.events.send((name, serde_json::to_value(payload)?));
        Ok(())
    }

    fn request_exit(&self) {
        self.0.host.request_exit();
    }

    /// The host stops admitting commands before closing. False means child
    /// termination could not be confirmed within the supervisor's deadline.
    pub async fn shutdown(&self) -> bool {
        *self
            .0
            .shutdown
            .get_or_init(|| async {
                process_lifecycle::begin_shutdown();
                let tasks = self.0.tasks.lock().unwrap().take();
                let drain = tokio::task::spawn_blocking(process_lifecycle::finish_shutdown);
                if let Some(mut tasks) = tasks {
                    tasks.shutdown().await;
                }
                drain.await.unwrap_or(false)
            })
            .await
    }
}
