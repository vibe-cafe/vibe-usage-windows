// Typed bridge to the Rust backend (invoke + events).

import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import {
  AppSettings,
  AppStatus,
  ProviderRateLimit,
  SyncState,
  UpdateInfo,
  UsageQuery,
  UsageResponse,
} from "./types";

export const api = {
  getAppStatus: () => invoke<AppStatus>("get_app_status"),

  fetchUsage: (query: UsageQuery) => invoke<UsageResponse>("fetch_usage", { query }),

  // Device link -------------------------------------------------------------
  /** Starts the flow; resolves with the user code once the browser is opened.
   *  Completion arrives via the `device-link` event. */
  startDeviceLink: () => invoke<{ userCode: string }>("start_device_link"),
  cancelDeviceLink: () => invoke<void>("cancel_device_link"),
  setManualKey: (apiKey: string) => invoke<void>("set_manual_key", { apiKey }),

  // Sync ---------------------------------------------------------------------
  triggerSync: () => invoke<void>("trigger_sync"),
  getSyncState: () => invoke<SyncState>("get_sync_state"),

  // Rate limits ---------------------------------------------------------------
  getRateLimits: (force: boolean) => invoke<ProviderRateLimit[]>("get_rate_limits", { force }),
  enableClaudeRateLimit: () => invoke<ProviderRateLimit[]>("enable_claude_rate_limit"),

  // Settings ------------------------------------------------------------------
  getSettings: () => invoke<AppSettings>("get_settings"),
  setSettings: (settings: AppSettings) => invoke<void>("set_settings", { settings }),
  getLaunchAtLogin: () => invoke<boolean>("get_launch_at_login"),
  setLaunchAtLogin: (enabled: boolean) => invoke<void>("set_launch_at_login", { enabled }),
  resetConfig: () => invoke<void>("reset_config"),

  // Windows / shell -------------------------------------------------------------
  openExternal: (url: string) => invoke<void>("open_external", { url }),
  openSettingsWindow: () => invoke<void>("open_settings_window"),
  hidePanel: () => invoke<void>("hide_panel"),
  quitApp: () => invoke<void>("quit_app"),

  // Updates -----------------------------------------------------------------------
  checkForUpdate: () => invoke<UpdateInfo | null>("check_for_update"),
  installUpdate: () => invoke<void>("install_update"),
};

// Events --------------------------------------------------------------------

export type DeviceLinkEvent =
  | { status: "success" }
  | { status: "denied" }
  | { status: "expired" }
  | { status: "error"; message: string };

export function onSyncState(handler: (s: SyncState) => void): Promise<UnlistenFn> {
  return listen<SyncState>("sync-state", (e) => handler(e.payload));
}

export function onDeviceLink(handler: (e: DeviceLinkEvent) => void): Promise<UnlistenFn> {
  return listen<DeviceLinkEvent>("device-link", (e) => handler(e.payload));
}

export function onUpdateAvailable(handler: (u: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("update-available", (e) => handler(e.payload));
}

/** Fired by Rust when the panel window is shown (popover-open refresh path). */
export function onPanelShown(handler: () => void): Promise<UnlistenFn> {
  return listen("panel-shown", () => handler());
}

/** Fired by Rust just before hiding so the frontend can play the close animation. */
export function onPanelWillHide(handler: () => void): Promise<UnlistenFn> {
  return listen("panel-will-hide", () => handler());
}
