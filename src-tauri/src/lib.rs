//! Vibe Usage for Windows — Tauri shell.
//! Windows port of the macOS Vibe Usage app (vibe-usage-app).

mod commands;
mod panel;
mod process_utils;
mod process_lifecycle;
mod services;
mod state;
mod tray;

use state::AppCtx;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let context = tauri::generate_context!();
    let app_config_dir = dirs::config_dir()
        .map(|dir| dir.join(&context.config().identifier))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let ctx = AppCtx::new(app_config_dir);
    commands::initialize_quota_selection(&ctx);
    tauri::Builder::default()
        .manage(ctx)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch → surface the main window.
            panel::show(app);
        }))
        .setup(|app| {
            tray::create_tray(app.handle())?;
            services::claude_usage::set_bundled_node(services::sync_engine::bundled_node(
                app.handle(),
            ));

            let handle = app.handle().clone();
            {
                let ctx = app.state::<AppCtx>();

                // New quota reads are inert subprocess/file reads. Undo only
                // the statusline edit older releases can prove they own.
                let retirement_handle = handle.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ =
                        services::rate_limits::statusline_hook(&retirement_handle).retire_legacy();
                });

                // Configured → immediate sync + 30-minute schedule.
                if ctx.config.is_configured() {
                    services::scheduler::start(handle.clone());
                }
            }
            services::scheduler::start_update_checks(handle);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the main window hides it to the tray instead of exiting.
            WindowEvent::CloseRequested { api, .. } if window.label() == panel::PANEL_LABEL => {
                api.prevent_close();
                panel::hide_now(window.app_handle());
            }
            // Settings is owned by the tray app. Hide it on close so the
            // keep-alive exit guard cannot leave the close action stuck.
            WindowEvent::CloseRequested { api, .. } if window.label() == "settings" => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::fetch_usage,
            commands::start_device_link,
            commands::cancel_device_link,
            commands::set_manual_key,
            commands::trigger_sync,
            commands::get_sync_state,
            commands::get_rate_limits,
            commands::enable_claude_rate_limit,
            commands::get_quota_products,
            commands::set_quota_product_selected,
            commands::get_zcode_credential_status,
            commands::set_zcode_quota_region,
            commands::set_zcode_api_key,
            commands::get_settings,
            commands::set_settings,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
            commands::get_extra_roots,
            commands::add_extra_root,
            commands::remove_extra_root,
            commands::reset_config,
            commands::open_external,
            commands::open_settings_window,
            commands::hide_panel,
            commands::quit_app,
            commands::export_test_diagnostics,
            commands::update_tray_stats,
            commands::check_for_update,
            commands::install_update,
        ])
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| {
            // Keep the tray app alive when every window is hidden/destroyed.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                } else if !process_lifecycle::shutdown_attempt_finished() {
                    // Explicit quit (UI, tray, updater): stop admitting CLI
                    // launches and wait for our child jobs before exiting.
                    api.prevent_exit();
                    if process_lifecycle::begin_shutdown() {
                        services::scheduler::stop(app);
                        let app = app.clone();
                        let code = code.unwrap_or(0);
                        tauri::async_runtime::spawn_blocking(move || {
                            if !process_lifecycle::finish_shutdown() {
                                log::warn!("CLI cleanup not confirmed before deadline; exiting with job kill-on-close");
                            }
                            app.exit(code);
                        });
                    }
                }
            }
        });
}
