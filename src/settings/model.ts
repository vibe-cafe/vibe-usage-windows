// Pure rules of the Settings window, ported from macOS
// `Views/Settings/SettingsModel.swift` (pages and navigation, the 同步 line and
// sidebar dot, the ten sync bars, reordering inside the 订阅配额 table). No
// React and no Tauri here, so every rule is unit-tested in Node.

import type { SyncRecord } from "../lib/diagnostics";
import type {
  AppStatus,
  CursorLoginStatus,
  ProviderRateLimit,
  RateLimitProvider,
  SyncState,
  ZCodeQuotaRegion,
} from "../lib/types";
import { formatRelativeTime } from "../lib/formatters";
import { cursorLoginRow } from "../lib/cursorLogin";

// MARK: - Pages

/** The five sidebar pages, in the order the maintainer set on 2026-11-19
 *  (「常规、订阅配额、数据同步、数据目录（高级）、关于」, 常规 later renamed 通用). */
export const SETTINGS_PAGES = ["general", "quotas", "sync", "directories", "about"] as const;
export type SettingsPage = (typeof SETTINGS_PAGES)[number];

export const SETTINGS_PAGE_TITLES: Record<SettingsPage, string> = {
  general: "通用",
  quotas: "订阅配额",
  sync: "数据同步",
  directories: "数据目录",
  about: "关于",
};

/**
 * Which page the window reopens on (Apple's guidance for settings windows,
 * macOS `SettingsNavigation.lastPageKey`). Kept in the settings WebView's own
 * `localStorage`: a UI-state key holding a page name only, read by nothing but
 * this window, so `settings.json` (shared with the panel) is untouched.
 */
export const LAST_PAGE_KEY = "settingsLastPage";

type PageStorage = Pick<Storage, "getItem" | "setItem">;

export function isSettingsPage(value: unknown): value is SettingsPage {
  return typeof value === "string" && (SETTINGS_PAGES as readonly string[]).includes(value);
}

/** First launch, an unknown value or storage that throws: 通用. */
export function restoredPage(storage: PageStorage | null | undefined): SettingsPage {
  try {
    const value = storage?.getItem(LAST_PAGE_KEY);
    return isSettingsPage(value) ? value : "general";
  } catch {
    return "general";
  }
}

export function rememberPage(storage: PageStorage | null | undefined, page: SettingsPage): void {
  try {
    storage?.setItem(LAST_PAGE_KEY, page);
  } catch {
    // UI state only: a full or blocked storage just forgets the page.
  }
}

// MARK: - Entry points

/**
 * What an entry point asks the window to show (`open_settings_window`'s
 * `focus`, taken once by the window as it shows):
 *
 * - none (the panel's 「设置」, the tray menu): wherever the user was last time;
 * - `quotas` (the gear at the end of the panel's tab strip): 订阅配额;
 * - `quota:<provider>` (a grey tab): 订阅配额, that row flashes;
 * - `cursorLogin` (the Cursor card's sign-in prompt): 订阅配额, the Cursor row
 *   flashes and opens on its sign-in.
 */
export type SettingsFocus = "quotas" | "cursorLogin" | `quota:${RateLimitProvider}`;

export const QUOTA_PROVIDERS: readonly RateLimitProvider[] = [
  "codex",
  "claudeCode",
  "kimi-code",
  "zcode",
  "grok",
  "cursor",
  "opencode-go",
  "antigravity",
];

export function quotaFocus(provider: RateLimitProvider): SettingsFocus {
  return `quota:${provider}`;
}

export type SettingsDestination =
  | { kind: "lastPage" }
  | { kind: "quotas"; provider: RateLimitProvider | null; signIn: boolean };

export function settingsDestination(focus: string | null | undefined): SettingsDestination {
  if (focus === "quotas") return { kind: "quotas", provider: null, signIn: false };
  if (focus === "cursorLogin") return { kind: "quotas", provider: "cursor", signIn: true };
  const provider = focus?.startsWith("quota:") ? focus.slice("quota:".length) : null;
  if (provider && (QUOTA_PROVIDERS as readonly string[]).includes(provider)) {
    return { kind: "quotas", provider: provider as RateLimitProvider, signIn: false };
  }
  return { kind: "lastPage" };
}

/**
 * Arriving at a row from the panel opens its configuration only while that is
 * what stands between the user and a card: ZCode without a key, Cursor while
 * it needs a sign-in. Turning a switch on does *not* open it. macOS
 * `SettingsNavigation.opensConfiguration`.
 */
export function opensConfiguration(
  provider: RateLimitProvider,
  zCodeKeyConfigured: boolean,
  cursorNeedsSignIn: boolean,
): boolean {
  if (provider === "zcode") return !zCodeKeyConfigured;
  if (provider === "cursor") return cursorNeedsSignIn;
  return false;
}

/** The Cursor row has something to open: the sign-in is on offer (the CLI
 *  has `login cursor`) or a sign-in is kept (macOS `CursorLogin.isAvailable`). */
export function cursorRowConfigurable(status: CursorLoginStatus | null | undefined): boolean {
  if (!status) return false;
  const row = cursorLoginRow(status);
  return row.text != null || row.action != null;
}

/** macOS `AppState.cursorSignInNeeded`: an ended sign-in, or no sign-in while
 *  the Cursor card asks for one. */
export function cursorSignInNeeded(
  status: CursorLoginStatus | null | undefined,
  cursorSnapshot: ProviderRateLimit | undefined,
): boolean {
  if (!status || !cursorRowConfigurable(status)) return false;
  if (status.state === "expired") return true;
  if (status.state !== "signedOut") return false;
  return cursorSnapshot?.provider === "cursor" && cursorSnapshot.signIn != null;
}

// MARK: - 数据同步

export interface SyncLine {
  kind: "ok" | "error" | null;
  text: string;
  spinning: boolean;
}

/** The 同步 row, macOS `SettingsSyncSummary.line`. */
export function syncLine(state: SyncState, configured: boolean, now: Date = new Date()): SyncLine {
  if (!configured) return { kind: null, text: "—", spinning: false };
  switch (state.status) {
    case "syncing":
      return { kind: null, text: "同步中…", spinning: true };
    case "error":
      return { kind: "error", text: state.message || "同步失败", spinning: false };
    default:
      if (state.lastSyncAt == null) return { kind: "ok", text: "正常", spinning: false };
      return {
        kind: "ok",
        text: `正常 · ${formatRelativeTime(new Date(state.lastSyncAt), now)}`,
        spinning: false,
      };
  }
}

/** Red after a failed sync while an account is linked; nothing otherwise
 *  (working, syncing, not linked) — the sidebar only speaks up when
 *  something needs attention. macOS `SettingsSyncSummary.sidebarDot`. */
export function syncSidebarDot(state: SyncState, configured: boolean): "error" | null {
  return configured && state.status === "error" ? "error" : null;
}

const SYNC_BAR_LIMIT = 10;

/** One bar per recorded sync, oldest on the left; `true` is a success. The
 *  app keeps the newest ten, newest first (`sync-history.json`). */
export function syncBars(records: readonly Pick<SyncRecord, "outcome">[]): boolean[] {
  return records
    .slice(0, SYNC_BAR_LIMIT)
    .reverse()
    .map((record) => record.outcome === "success");
}

// MARK: - 订阅配额 reordering

export type QuotaReorderTarget =
  | { kind: "before"; provider: RateLimitProvider | null }
  | { kind: "reject" };

/**
 * A drop on `row` (its lower half when `dropAfter`). Drops stay inside the
 * dragged product's own group (on / off), like the panel's strip; the result
 * is `moveQuotaProduct`'s `target` (`null` = the end of the dragged group).
 * macOS `QuotaReorder.target`.
 */
export function quotaReorderTarget(
  dragged: RateLimitProvider | null,
  row: RateLimitProvider,
  dropAfter: boolean,
  enabled: readonly RateLimitProvider[],
  disabled: readonly RateLimitProvider[],
): QuotaReorderTarget {
  if (!dragged) return { kind: "reject" };
  const group = enabled.includes(row) ? enabled : disabled;
  const index = group.indexOf(row);
  if (!group.includes(dragged) || index < 0) return { kind: "reject" };
  if (!dropAfter) return { kind: "before", provider: row };
  return { kind: "before", provider: group[index + 1] ?? null };
}

// MARK: - ZCode

/** Same names as macOS `ZCodeQuotaRegion.displayName` / `apiKeyName`. */
export const ZCODE_REGIONS: { value: ZCodeQuotaRegion; title: string; keyName: string }[] = [
  { value: "bigModel", title: "BigModel（国内）", keyName: "BigModel API Key" },
  { value: "zAI", title: "Z.ai（海外）", keyName: "Z.ai API Key" },
];

// MARK: - 关于

/** 「0.7.10」, plus 「· 外测」 in external-test builds (macOS `SettingsVersion.full`;
 *  Windows has no separate build number). */
export function settingsVersionText(status: Pick<AppStatus, "version" | "updatesAvailable"> | null): string {
  if (!status) return "";
  return status.updatesAvailable ? status.version : `${status.version} · 外测`;
}

/** Public repositories on the 关于 page; the VibeCafé site follows the
 *  configured server (`AppStatus.apiUrl`). */
export const SETTINGS_LINKS = {
  cliRepository: "https://github.com/vibe-cafe/vibe-usage",
  appRepository: "https://github.com/vibe-cafe/vibe-usage-windows",
} as const;

/** The site's origin for the 链接 row; null when the configured address is not http(s). */
export function siteUrl(apiUrl: string | null | undefined): string | null {
  if (!apiUrl) return null;
  try {
    const url = new URL(apiUrl);
    return url.protocol === "https:" || url.protocol === "http:" ? url.origin : null;
  } catch {
    return null;
  }
}
