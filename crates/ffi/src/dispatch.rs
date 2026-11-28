use super::{Error, Reply};
use vibe_backend::{
    commands as c,
    serde::{de::DeserializeOwned, Serialize},
    serde_json::{self, Map, Value},
    Backend,
};

fn field<T: DeserializeOwned>(args: &mut Map<String, Value>, key: &str) -> Reply<T> {
    serde_json::from_value(args.remove(key).ok_or(Error::Arguments)?).map_err(|_| Error::Arguments)
}

fn encode<T: Serialize>(value: T) -> Reply<Value> {
    serde_json::to_value(value).map_err(|_| Error::Internal)
}

pub async fn run(backend: Backend, name: &str, json: &str) -> Reply<Value> {
    dispatch(
        backend,
        name,
        json,
        #[cfg(test)]
        None,
    )
    .await
}

// A contract test substitutes only the result of an operation with external
// side effects. Both branches must have the same concrete backend type.
macro_rules! outcome {
    ($body:expr, $sample:ident) => {
        $body
    };
    ($body:expr, $sample:ident, |$v:ident| $stub:expr) => {{
        #[cfg(test)]
        let value = if let Some($v) = $sample { $stub } else { $body };
        #[cfg(not(test))]
        let value = $body;
        value
    }};
}

macro_rules! commands {
    ($($name:ident ($($arg:ident : $key:literal),*) => |$app:ident| $body:expr $(, |$v:ident| $stub:expr)?;)+) => {
        #[cfg(test)]
        pub const NAMES: &[&str] = &[$(stringify!($name)),+];

        pub(super) async fn dispatch(backend: Backend, name: &str, json: &str,
            #[cfg(test)] sample: Option<Value>) -> Reply<Value> {
            let mut args: Map<String, Value> =
                serde_json::from_str(json).map_err(|_| Error::Arguments)?;
            match name {
                $(stringify!($name) => {
                    $(let $arg = field(&mut args, $key)?;)*
                    if !args.is_empty() { return Err(Error::Arguments); }
                    let $app = backend;
                    encode(outcome!($body, sample $(, |$v| $stub)?))
                })+
                _ => Err(Error::Unknown),
            }
        }
    }
}

commands! {
    get_app_status() => |app| c::get_app_status(app),
        |_fixture| c::wire_fixtures::app_status();
    fetch_usage(query: "query") => |app| c::fetch_usage(app, query).await?,
        |sample| sample;
    cached_usage() => |app| app.cached_usage(),
        |sample| Some(vibe_backend::UsageSnapshot {
            query: serde_json::from_value(sample["query"].clone()).unwrap(),
            data: sample["data"].clone(),
        });
    panel_opened(query: "query") => |app| app.panel_opened(query),
        |_fixture| ();
    settings_shown() => |app| app.settings_shown(),
        |_fixture| ();
    start_device_link() => |app| c::start_device_link(app).await?,
        |_fixture| c::wire_fixtures::device_code();
    cancel_device_link() => |app| c::cancel_device_link(app);
    trigger_sync() => |app| c::trigger_sync(app),
        |_fixture| ();
    collect_diagnostics() => |app| c::collect_diagnostics(app).await,
        |_fixture| "合成诊断摘要".to_owned();
    get_sync_state() => |app| c::get_sync_state(app);
    get_rate_limits(force: "force") => |app| c::get_rate_limits(app, force).await,
        |sample| serde_json::from_value(sample).unwrap();
    get_cached_rate_limits() => |app| c::get_cached_rate_limits(app);
    get_quota_products() => |app| c::get_quota_products(app),
        |_fixture| c::wire_fixtures::products();
    set_quota_product_selected(provider: "provider", selected: "selected") => |app| c::set_quota_product_selected(app, provider, selected).await?,
        |sample| serde_json::from_value(sample).unwrap();
    get_zcode_credential_status() => |_app| c::get_zcode_credential_status()?,
        |_fixture| c::wire_fixtures::credentials();
    set_zcode_quota_region(region: "region") => |app| c::set_zcode_quota_region(app, region).await?,
        |sample| serde_json::from_value(sample).unwrap();
    set_zcode_api_key(region: "region", key: "apiKey") => |app| c::set_zcode_api_key(app, region, key).await?,
        |sample| serde_json::from_value(sample).unwrap();
    get_cursor_login() => |app| c::get_cursor_login(app),
        |_fixture| c::wire_fixtures::login(false);
    start_cursor_login() => |app| c::start_cursor_login(app),
        |_fixture| c::wire_fixtures::login(true);
    cancel_cursor_login() => |app| c::cancel_cursor_login(app),
        |_fixture| c::wire_fixtures::login(false);
    logout_cursor() => |app| c::logout_cursor(app)?,
        |_fixture| c::wire_fixtures::login(false);
    get_settings() => |app| c::get_settings(app);
    update_setting(change: "change") => |app| c::update_setting(app, change)?;
    get_sync_history() => |app| c::get_sync_history(app);
    get_launch_at_login() => |_app| c::get_launch_at_login()?,
        |_fixture| false;
    set_launch_at_login(enabled: "enabled") => |_app| c::set_launch_at_login(enabled)?,
        |_fixture| ();
    get_extra_roots() => |app| c::get_extra_roots(app).await?,
        |_fixture| serde_json::json!({"codex":["fixture/codex"]});
    add_extra_root(source: "source", path: "path") => |app| c::add_extra_root(app, source, path).await?,
        |_fixture| ();
    remove_extra_root(source: "source", path: "path") => |app| c::remove_extra_root(app, source, path).await?,
        |_fixture| ();
    reset_config() => |app| c::reset_config(app)?;
    open_external(url: "url") => |_app| c::open_external(url)?,
        |_fixture| ();
    export_test_diagnostics(destination: "destination") => |app| c::export_test_diagnostics(app, destination)?,
        |_fixture| ();
    check_for_update() => |app| c::check_for_update(app).await?,
        |_fixture| Some(vibe_backend::UpdateInfo {
            version: "0.8.1".into(), url: "https://example.invalid/update".into(), notes: None, sha256: None,
        });
    get_update_info() => |app| c::get_update_info(app);
    install_update() => |app| c::install_update(app).await?,
        |_fixture| ();
}
