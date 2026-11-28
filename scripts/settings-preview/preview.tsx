// Off-screen preview of the Settings window for design review (macOS has the
// same in `SettingsRenderTests`). The real `SettingsApp` runs in a browser
// with every Tauri command answered from synthetic fixtures through
// `@tauri-apps/api/mocks`: no backend, no network, no real account, no
// files. `render.mjs` loads one state per URL (`?state=<name>`) in headless
// Chrome and saves a PNG.

import React from "react";
import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import "../../src/styles/globals.css";
import { SettingsApp } from "../../src/SettingsApp";
import type {
  AppSettings,
  AppStatus,
  CursorLoginStatus,
  ExtraRoots,
  ProviderRateLimit,
  QuotaProduct,
  RateLimitProvider,
  SyncState,
} from "../../src/lib/types";

const params = new URLSearchParams(window.location.search);
const stateName = params.get("state") ?? "general";

const NOW = Date.now();
const ORDER: RateLimitProvider[] = [
  "claudeCode", "codex", "cursor", "grok", "antigravity", "kimi-code", "zcode", "opencode-go",
];
const NAMES: Record<RateLimitProvider, string> = {
  codex: "Codex",
  claudeCode: "Claude",
  "kimi-code": "Kimi Code",
  zcode: "ZCode",
  grok: "Grok",
  cursor: "Cursor",
  "opencode-go": "OpenCode Go",
  antigravity: "Antigravity / AGY",
};

interface Fixture {
  page: string;
  focus?: string;
  configured: boolean;
  selected: RateLimitProvider[];
  zCodeKey: boolean;
  cursor: CursorLoginStatus;
  snapshots: ProviderRateLimit[];
  sync: SyncState;
  failures: number;
  extraRoots: ExtraRoots;
  zCodeSaveError?: string;
  /** DOM steps once the window has loaded: click a labelled control, type. */
  actions: Action[];
}

type Action =
  | { click: string }
  | { type: string; into: string }
  | { drag: RateLimitProvider; to: RateLimitProvider; after: boolean };

const HOME = "C:\\Users\\name";
const linked = (failures = 0): Partial<Fixture> => ({
  configured: true,
  sync: failures
    ? { status: "error", message: "API Key 无效，请重新配置", lastSyncAt: NOW - 26 * 60_000 }
    : { status: "idle", lastSyncAt: NOW - 26 * 60_000 },
  failures,
});

const BASE: Fixture = {
  page: "general",
  configured: false,
  selected: ["claudeCode", "codex", "cursor", "grok"],
  zCodeKey: false,
  cursor: { state: "signedOut", supported: true },
  snapshots: [],
  sync: { status: "idle" },
  failures: 0,
  extraRoots: {},
  actions: [],
};

const STATES: Record<string, Partial<Fixture>> = {
  general: { page: "general" },
  quotas: { page: "quotas" },
  "quotas-zcode": { page: "quotas", actions: [{ click: "展开 ZCode" }] },
  "quotas-zcode-saved": { page: "quotas", zCodeKey: true, actions: [{ click: "展开 ZCode" }] },
  "quotas-zcode-error": {
    page: "quotas",
    zCodeSaveError: "无法写入 Windows 凭据管理器（拒绝访问）",
    actions: [{ click: "展开 ZCode" }, { type: "fixture-key-0000000000", into: "粘贴 API Key" }, { click: "保存" }],
  },
  "quotas-cursor": { page: "quotas", actions: [{ click: "展开 Cursor" }] },
  "quotas-cursor-signed-in": {
    page: "quotas",
    cursor: { state: "signedIn", email: "name@example.com", supported: true },
    actions: [{ click: "展开 Cursor" }],
  },
  "quotas-cursor-expired": {
    page: "quotas",
    cursor: { state: "expired", email: "name@example.com", supported: true },
    actions: [{ click: "展开 Cursor" }],
  },
  "quotas-status": {
    page: "quotas",
    selected: ["claudeCode", "codex", "cursor", "grok", "antigravity", "kimi-code"],
    snapshots: [
      { provider: "codex", status: { kind: "unauthorized" }, fetchedAt: NOW / 1000 },
      { provider: "cursor", status: { kind: "noData" }, fetchedAt: NOW / 1000, signIn: "expired" },
      { provider: "grok", status: { kind: "ok" }, fetchedAt: NOW / 1000, liveFailure: { code: "egressRegionUnsupported", region: "CN" } },
      { provider: "antigravity", status: { kind: "retryableError" }, fetchedAt: NOW / 1000, liveFailure: { code: "cliTimedOut" } },
      { provider: "kimi-code", status: { kind: "error", message: "fixture" }, fetchedAt: NOW / 1000 },
    ],
  },
  "quotas-menu": { page: "quotas", actions: [{ click: "没检测到代理时" }] },
  "quotas-drag": { page: "quotas", actions: [{ drag: "grok", to: "claudeCode", after: false }] },
  "quotas-from-panel": { page: "general", focus: "quota:zcode" },
  sync: { page: "sync", ...linked() },
  "sync-error": { page: "sync", ...linked(2) },
  "sync-relinking": { page: "sync", ...linked(), actions: [{ click: "重新链接" }] },
  "sync-unlinked": { page: "sync" },
  "sync-reset": { page: "sync", ...linked(), actions: [{ click: "重置…" }] },
  directories: { page: "directories", extraRoots: { codex: [`${HOME}\\work\\sandbox-a\\.codex`] } },
  "directories-empty": { page: "directories" },
  "directories-roots": {
    page: "directories",
    extraRoots: {
      codex: [
        `${HOME}\\work\\sandbox-a\\.codex`,
        `${HOME}\\AppData\\Local\\agents\\workspace-0042\\sandbox\\profile-3\\.codex`,
      ],
      grok: [`${HOME}\\work\\sandbox-a\\.grok`],
    },
  },
  about: { page: "about" },
  "about-copied": { page: "about", actions: [{ click: "复制" }] },
};

const fixture: Fixture = { ...BASE, ...STATES[stateName] };
window.localStorage.setItem("settingsLastPage", fixture.page);

const products: QuotaProduct[] = (
  ["codex", "claudeCode", "kimi-code", "zcode", "grok", "cursor", "opencode-go", "antigravity"] as RateLimitProvider[]
).map((provider) => ({
  provider,
  displayName: NAMES[provider],
  availability: "ready",
  isDetected: provider !== "opencode-go",
}));

let settings: AppSettings = {
  showCostInTray: true,
  showTokensInTray: false,
  codexRateLimitEnabled: true,
  claudeRateLimitEnabled: true,
  selectedQuotaProductIds: fixture.selected,
  quotaProductOrder: ORDER,
  quotaSelectedTabId: null,
  quotaSelectionInitialized: true,
  zCodeQuotaRegion: "bigModel",
  egressMode: "auto",
};

const status: AppStatus = {
  configured: fixture.configured,
  hostname: "fixture-pc",
  apiUrl: "https://vibefriends.com",
  version: "0.7.10",
  isDev: false,
  runtimeAvailable: true,
  testDiagnosticsAvailable: true,
  updatesAvailable: true,
  apiKeyDisplay: fixture.configured ? "vbu_k3f9...a41c" : null,
};

const history = Array.from({ length: fixture.configured ? 10 : 0 }, (_, index) => ({
  startedAt: NOW - index * 1_800_000,
  trigger: "scheduled",
  outcome: index < fixture.failures ? "failed" : "success",
  durationMs: 4_000,
}));

const report = {
  schemaVersion: 1,
  generatedAt: NOW,
  app: { version: "0.7.10", build: "release" },
  system: { windowsRelease: 11, displayVersion: "24H2", build: 26100, architecture: "x86_64" },
  runtime: { kind: "bundledNode", version: "22.23.1" },
  cli: { launcherPresent: true, cachedVersion: "0.15.10" },
  account: { keyConfigured: true, lastSyncKey: "accepted" },
  deviceTag: "7c2e",
  network: { systemProxy: "loopback", systemProxyPassedToSync: true, autoConfigScript: false, variables: [] },
  scheduler: { running: true, syncing: false, lastSuccessAt: NOW - 26 * 60_000 },
  syncs: [{ startedAt: NOW - 26 * 60_000, trigger: "scheduled", outcome: "success", durationMs: 4_000 }],
  quotas: [{ provider: "claudeCode", status: "ok", source: "live", meterCount: 2 }],
  claude: {
    selected: true,
    lastProbe: null,
    candidates: [],
    pathMatches: [],
    userVariables: { names: [], other: 0 },
    settings: { kind: "read", env: { names: [], other: 0 } },
    configDirSet: false,
  },
};

let focus: string | null = fixture.focus ?? null;
let zCodeKey = fixture.zCodeKey;

mockIPC(
  (cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "plugin:app|name":
        return "Vibe Usage";
      case "take_settings_focus": {
        const taken = focus;
        focus = null;
        return taken;
      }
      case "get_quota_products":
        return products;
      case "get_app_status":
        return status;
      case "get_settings":
        return settings;
      case "set_settings":
        settings = payload.settings as AppSettings;
        return null;
      case "get_sync_state":
        return fixture.sync;
      case "get_sync_history":
        return history;
      case "get_launch_at_login":
        return false;
      case "get_extra_roots":
        return fixture.extraRoots;
      case "get_zcode_credential_status":
        return { bigModelConfigured: zCodeKey, zAiConfigured: false };
      case "set_zcode_api_key":
        if (fixture.zCodeSaveError) throw fixture.zCodeSaveError;
        zCodeKey = payload.apiKey != null;
        return [];
      case "get_cursor_login":
        return fixture.cursor;
      case "get_cached_rate_limits":
        return { snapshots: fixture.snapshots, refreshing: false };
      case "set_quota_product_selected": {
        const provider = payload.provider as RateLimitProvider;
        const ids = settings.selectedQuotaProductIds.filter((id) => id !== provider);
        settings = { ...settings, selectedQuotaProductIds: payload.selected ? [...ids, provider] : ids };
        return [];
      }
      case "start_device_link":
        return { userCode: "WDJB-MJHT" };
      case "get_update_info":
        return null;
      case "collect_diagnostics":
        return report;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

// Headless Chrome has no clipboard permission; the real app's WebView does.
Object.defineProperty(navigator, "clipboard", {
  value: { writeText: async () => {} },
  configurable: true,
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SettingsApp />
  </React.StrictMode>,
);

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function byLabel(label: string): HTMLElement | null {
  const candidates = Array.from(document.querySelectorAll<HTMLElement>("button, input, [role=switch]"));
  return (
    candidates.find((element) => element.getAttribute("aria-label") === label) ??
    candidates.find((element) => element.textContent?.trim() === label) ??
    null
  );
}

function rowOf(provider: RateLimitProvider): HTMLElement | null {
  const toggle = byLabel(NAMES[provider]);
  return toggle?.closest<HTMLElement>("[style*='height: 40px']") ?? null;
}

async function run() {
  await wait(400);
  for (const action of fixture.actions) {
    if ("click" in action) {
      byLabel(action.click)?.click();
    } else if ("type" in action) {
      const input = byLabel(action.into) as HTMLInputElement | null;
      if (input) {
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
        setter.call(input, action.type);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      }
    } else {
      const from = rowOf(action.drag);
      const to = rowOf(action.to);
      if (from && to) {
        const a = from.getBoundingClientRect();
        const b = to.getBoundingClientRect();
        const init = { bubbles: true, pointerId: 7, button: 0, clientX: a.left + 120 };
        from.dispatchEvent(new PointerEvent("pointerdown", { ...init, clientY: a.top + 20 }));
        await wait(30);
        const y = action.after ? b.bottom - 6 : b.top + 6;
        from.dispatchEvent(new PointerEvent("pointermove", { ...init, clientY: a.top + 14 }));
        await wait(30);
        from.dispatchEvent(new PointerEvent("pointermove", { ...init, clientY: y }));
      }
    }
    await wait(250);
  }
  document.body.dataset.ready = "1";
}
void run();
