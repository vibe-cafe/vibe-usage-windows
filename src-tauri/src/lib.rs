//! Vibe Usage for Windows — Tauri shell.
//! Windows port of the macOS Vibe Usage app (vibe-usage-app).

mod commands;
mod panel;
mod process_utils;
mod services;
mod state;
mod tray;

use state::AppCtx;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch → surface the panel (mirrors Dock-reopen behavior).
            panel::show(app, None);
        }))
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            app.manage(AppCtx::new(config_dir));

            tray::create_tray(app.handle())?;

            let handle = app.handle().clone();
            {
                let ctx = app.state::<AppCtx>();

                // Self-heal the Claude statusline wrapper if it was clobbered
                // (mirrors AppState.initialize → StatuslineHook.verifyAndRepair).
                let claude_enabled = ctx.settings.lock().unwrap().claude_rate_limit_enabled;
                if claude_enabled {
                    let repair_handle = handle.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        services::rate_limits::statusline_hook(&repair_handle)
                            .verify_and_repair(true);
                    });
                }

                // Configured → immediate sync + 30-minute schedule.
                if ctx.config.is_configured() {
                    services::scheduler::start(handle.clone());
                }
            }
            services::scheduler::start_update_checks(handle);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Click-outside dismiss: hide the panel when it loses focus.
            WindowEvent::Focused(false) if window.label() == panel::PANEL_LABEL => {
                panel::begin_hide(window.app_handle());
            }
            // Closing the popover must never destroy it — hide instead.
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
            commands::get_settings,
            commands::set_settings,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
            commands::reset_config,
            commands::open_external,
            commands::open_settings_window,
            commands::hide_panel,
            commands::quit_app,
            commands::update_tray_stats,
            commands::check_for_update,
            commands::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Keep the tray app alive when every window is hidden/destroyed.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
