// Typed bridge to the Rust backend (invoke + events).

import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type { DiagnosticsReport } from "./diagnostics";
import {
  AppSettings,
  AppStatus,
  CursorLoginStatus,
  ExtraRoots,
  ProviderRateLimit,
  QuotaProduct,
  RateLimitProvider,
  SyncState,
  UpdateInfo,
  UsageQuery,
  UsageResponse,
  ZCodeCredentialStatus,
  ZCodeQuotaRegion,
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
  getCachedRateLimits: () =>
    invoke<{ snapshots: ProviderRateLimit[]; refreshing: boolean }>("get_cached_rate_limits"),
  enableClaudeRateLimit: () => invoke<ProviderRateLimit[]>("enable_claude_rate_limit"),
  getQuotaProducts: () => invoke<QuotaProduct[]>("get_quota_products"),
  setQuotaProductSelected: (provider: RateLimitProvider, selected: boolean) =>
    invoke<ProviderRateLimit[]>("set_quota_product_selected", { provider, selected }),
  getZCodeCredentialStatus: () =>
    invoke<ZCodeCredentialStatus>("get_zcode_credential_status"),
  setZCodeQuotaRegion: (region: ZCodeQuotaRegion) =>
    invoke<ProviderRateLimit[]>("set_zcode_quota_region", { region }),
  setZCodeApiKey: (region: ZCodeQuotaRegion, apiKey: string | null) =>
    invoke<ProviderRateLimit[]>("set_zcode_api_key", { region, apiKey }),

  // Cursor sign-in --------------------------------------------------------------
  /** Also asks the CLI whether it offers the sign-in; the answer arrives
   *  with `cursor-login-updated`. */
  getCursorLogin: () => invoke<CursorLoginStatus>("get_cursor_login"),
  startCursorLogin: () => invoke<CursorLoginStatus>("start_cursor_login"),
  cancelCursorLogin: () => invoke<CursorLoginStatus>("cancel_cursor_login"),
  logoutCursor: () => invoke<CursorLoginStatus>("logout_cursor"),

  // Settings ------------------------------------------------------------------
  getSettings: () => invoke<AppSettings>("get_settings"),
  setSettings: (settings: AppSettings) => invoke<void>("set_settings", { settings }),
  getLaunchAtLogin: () => invoke<boolean>("get_launch_at_login"),
  setLaunchAtLogin: (enabled: boolean) => invoke<void>("set_launch_at_login", { enabled }),
  getExtraRoots: () => invoke<ExtraRoots>("get_extra_roots"),
  addExtraRoot: (source: string, path: string) =>
    invoke<void>("add_extra_root", { source, path }),
  removeExtraRoot: (source: string, path: string) =>
    invoke<void>("remove_extra_root", { source, path }),
  resetConfig: () => invoke<void>("reset_config"),

  // Windows / shell -------------------------------------------------------------
  openExternal: (url: string) => invoke<void>("open_external", { url }),
  /** `focus`: scroll the settings to that part once shown. */
  openSettingsWindow: (focus?: SettingsFocus) =>
    invoke<void>("open_settings_window", focus ? { focus } : {}),
  takeSettingsFocus: () => invoke<SettingsFocus | null>("take_settings_focus"),
  hidePanel: () => invoke<void>("hide_panel"),
  quitApp: () => invoke<void>("quit_app"),
  exportTestDiagnostics: (destination: string) =>
    invoke<void>("export_test_diagnostics", { destination }),

  /** 「复制诊断信息」: whitelisted typed fields only; laid out by lib/diagnostics. */
  collectDiagnostics: () => invoke<DiagnosticsReport>("collect_diagnostics"),

  // Updates -----------------------------------------------------------------------
  checkForUpdate: () => invoke<UpdateInfo | null>("check_for_update"),
  /** What the backend's latest check found; never starts a check. */
  getUpdateInfo: () => invoke<UpdateInfo | null>("get_update_info"),
  installUpdate: () => invoke<void>("install_update"),
};

export type SettingsFocus = "cursorLogin";

// Events --------------------------------------------------------------------

/** The Cursor sign-in changed: signed in or out, waiting, ended, or the CLI's
 *  support became known. */
export function onCursorLogin(handler: (status: CursorLoginStatus) => void): Promise<UnlistenFn> {
  return listen<CursorLoginStatus>("cursor-login-updated", (e) => handler(e.payload));
}

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

/** A later check found no newer version (e.g. the release was pulled): drop the offer. */
export function onUpdateCleared(handler: () => void): Promise<UnlistenFn> {
  return listen("update-cleared", () => handler());
}

export function onSettingsUpdated(handler: (settings: AppSettings) => void): Promise<UnlistenFn> {
  return listen<AppSettings>("settings-updated", (e) => handler(e.payload));
}

/** Fired whenever the already-loaded settings window is made visible. */
export function onSettingsShown(handler: () => void): Promise<UnlistenFn> {
  return listen("settings-shown", () => handler());
}

/** A null payload hides the previous warning while the panel is refreshing. */
export function observeCachedRateLimits(update: (snapshots: ProviderRateLimit[] | null) => void): UnlistenFn {
  let active = true;
  let revision = 0;
  async function load() {
    const request = ++revision;
    try {
      const cached = await api.getCachedRateLimits();
      if (active && request === revision) update(cached.refreshing ? null : cached.snapshots);
    } catch {
      // Keep the last result; the next panel event can still update it.
    }
  }
  const subs = Promise.all([
    listen<ProviderRateLimit[] | null>("rate-limits-updated", ({ payload }) => {
      revision++;
      if (active) update(payload);
    }),
    onSettingsShown(load),
  ]);
  // Subscribe first. A panel update outranks an older command response.
  void subs.then(() => { if (active) void load(); });
  return () => {
    active = false;
    void subs.then((listeners) => listeners.forEach((unlisten) => unlisten()));
  };
}

/** Fired by Rust when the panel window is shown (popover-open refresh path). */
export function onPanelShown(handler: () => void): Promise<UnlistenFn> {
  return listen("panel-shown", () => handler());
}


/** Fired by Rust just before hiding so the frontend can play the close animation. */
export function onPanelWillHide(handler: () => void): Promise<UnlistenFn> {
  return listen("panel-will-hide", () => handler());
}
