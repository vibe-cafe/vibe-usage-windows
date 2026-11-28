import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { moveQuotaProduct, quotaProductStatus, quotaTabOrder } from "../src/lib/quotaProducts";
import type { CursorLoginStatus, QuotaProduct, RateLimitProvider } from "../src/lib/types";
import { SettingsContext, type SettingsModel } from "../src/settings/context";
import { SettingsWindow } from "../src/settings/SettingsWindow";
import {
  LAST_PAGE_KEY,
  SETTINGS_PAGES,
  SETTINGS_PAGE_TITLES,
  cursorRowConfigurable,
  cursorSignInNeeded,
  opensConfiguration,
  quotaFocus,
  quotaReorderTarget,
  rememberPage,
  restoredPage,
  settingsDestination,
  settingsVersionText,
  siteUrl,
  syncBars,
  syncLine,
  syncSidebarDot,
  type SettingsPage,
} from "../src/settings/model";

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
}

describe("pages and the remembered page", () => {
  it("has macOS's five pages in the maintainer's order", () => {
    expect(SETTINGS_PAGES.map((page) => SETTINGS_PAGE_TITLES[page])).toEqual([
      "通用",
      "订阅配额",
      "数据同步",
      "数据目录",
      "关于",
    ]);
  });

  it("opens on 通用 the first time and on the last page after that", () => {
    expect(restoredPage(memoryStorage())).toBe("general");
    expect(restoredPage(null)).toBe("general");
    expect(restoredPage(memoryStorage({ [LAST_PAGE_KEY]: "sync" }))).toBe("sync");
    expect(restoredPage(memoryStorage({ [LAST_PAGE_KEY]: "dock" }))).toBe("general");
    const throwing = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(restoredPage(throwing)).toBe("general");
    expect(() => rememberPage(throwing, "about")).not.toThrow();
    const storage = memoryStorage();
    rememberPage(storage, "directories");
    expect(storage.values.get(LAST_PAGE_KEY)).toBe("directories");
  });
});

describe("entry points from the panel (G18)", () => {
  it("keeps the last page for 「设置」 and anything it does not know", () => {
    expect(settingsDestination(null)).toEqual({ kind: "lastPage" });
    expect(settingsDestination("about")).toEqual({ kind: "lastPage" });
    expect(settingsDestination("quota:")).toEqual({ kind: "lastPage" });
    expect(settingsDestination("quota:unknown")).toEqual({ kind: "lastPage" });
  });

  it("lands the gear on 订阅配额 and a grey tab on its own row", () => {
    expect(settingsDestination("quotas")).toEqual({ kind: "quotas", provider: null, signIn: false });
    expect(settingsDestination(quotaFocus("zcode"))).toEqual({ kind: "quotas", provider: "zcode", signIn: false });
    expect(settingsDestination(quotaFocus("kimi-code"))).toEqual({
      kind: "quotas",
      provider: "kimi-code",
      signIn: false,
    });
    expect(settingsDestination("cursorLogin")).toEqual({ kind: "quotas", provider: "cursor", signIn: true });
  });

  it("opens a row only when its configuration is what blocks a card", () => {
    expect(opensConfiguration("zcode", false, false)).toBe(true);
    expect(opensConfiguration("zcode", true, false)).toBe(false);
    expect(opensConfiguration("cursor", false, true)).toBe(true);
    expect(opensConfiguration("cursor", false, false)).toBe(false);
    expect(opensConfiguration("codex", false, true)).toBe(false);
  });

  it("knows when the Cursor row has something to open and when it needs a sign-in", () => {
    const status = (state: CursorLoginStatus["state"], supported: boolean | null): CursorLoginStatus => ({
      state,
      supported,
    });
    expect(cursorRowConfigurable(null)).toBe(false);
    expect(cursorRowConfigurable(status("signedOut", false))).toBe(false);
    expect(cursorRowConfigurable(status("signedOut", true))).toBe(true);
    expect(cursorRowConfigurable(status("signedIn", false))).toBe(true);

    const askedFor = { provider: "cursor" as const, status: { kind: "noData" as const }, signIn: "required" as const };
    expect(cursorSignInNeeded(status("expired", true), undefined)).toBe(true);
    expect(cursorSignInNeeded(status("signedOut", true), askedFor)).toBe(true);
    expect(cursorSignInNeeded(status("signedOut", true), undefined)).toBe(false);
    expect(cursorSignInNeeded(status("signedIn", true), askedFor)).toBe(false);
    expect(cursorSignInNeeded(status("signedOut", false), askedFor)).toBe(false);
  });
});

describe("数据同步 summary", () => {
  const now = new Date(2026, 10, 28, 15, 0);

  it("words the 同步 row like macOS", () => {
    expect(syncLine({ status: "error", message: "x" }, false, now)).toEqual({ kind: null, text: "—", spinning: false });
    expect(syncLine({ status: "syncing" }, true, now)).toEqual({ kind: null, text: "同步中…", spinning: true });
    expect(syncLine({ status: "error", message: "API Key 无效，请重新配置" }, true, now)).toEqual({
      kind: "error",
      text: "API Key 无效，请重新配置",
      spinning: false,
    });
    expect(syncLine({ status: "idle" }, true, now).text).toBe("正常");
    expect(syncLine({ status: "success", lastSyncAt: now.getTime() - 26 * 60_000 }, true, now).text).toBe(
      "正常 · 26 分钟前",
    );
  });

  it("puts the red dot on the sidebar only after a failed sync of a linked account", () => {
    expect(syncSidebarDot({ status: "error" }, true)).toBe("error");
    expect(syncSidebarDot({ status: "error" }, false)).toBeNull();
    expect(syncSidebarDot({ status: "syncing" }, true)).toBeNull();
    expect(syncSidebarDot({ status: "success" }, true)).toBeNull();
  });

  it("draws the newest ten runs oldest first, anything but success as a failure", () => {
    const outcomes = ["timedOut", "success", "failed", "cliStartupFailed", ...Array(8).fill("success")] as const;
    const records = outcomes.map((outcome) => ({ outcome }));
    const bars = syncBars(records);
    expect(bars).toHaveLength(10);
    expect(bars.slice(-4)).toEqual([false, false, true, false]);
    expect(syncBars([])).toEqual([]);
  });
});

describe("订阅配额 reordering (G17)", () => {
  const enabled: RateLimitProvider[] = ["claudeCode", "codex", "cursor"];
  const disabled: RateLimitProvider[] = ["grok", "zcode"];

  it("drops before a row, or before the next one from its lower half", () => {
    expect(quotaReorderTarget("cursor", "claudeCode", false, enabled, disabled)).toEqual({
      kind: "before",
      provider: "claudeCode",
    });
    expect(quotaReorderTarget("claudeCode", "codex", true, enabled, disabled)).toEqual({
      kind: "before",
      provider: "cursor",
    });
    expect(quotaReorderTarget("claudeCode", "cursor", true, enabled, disabled)).toEqual({
      kind: "before",
      provider: null,
    });
  });

  it("keeps a drop inside the dragged product's own group", () => {
    expect(quotaReorderTarget("grok", "codex", false, enabled, disabled)).toEqual({ kind: "reject" });
    expect(quotaReorderTarget("codex", "zcode", true, enabled, disabled)).toEqual({ kind: "reject" });
    expect(quotaReorderTarget(null, "codex", false, enabled, disabled)).toEqual({ kind: "reject" });
  });

  it("writes the same stored order the panel's tab strip reads", () => {
    const catalog: RateLimitProvider[] = ["codex", "claudeCode", "kimi-code", "zcode", "grok", "cursor"];
    const selected: RateLimitProvider[] = ["codex", "cursor"];
    const stored: RateLimitProvider[] = ["cursor", "zcode", "codex", "grok"];
    const order = quotaTabOrder(catalog, selected, stored);
    expect(order).toEqual(["cursor", "codex", "zcode", "grok", "claudeCode", "kimi-code"]);
    // Drag 「Kimi Code」 to the top of 未开启.
    const target = quotaReorderTarget(
      "kimi-code",
      "zcode",
      false,
      order.filter((item) => selected.includes(item)),
      order.filter((item) => !selected.includes(item)),
    );
    expect(target).toEqual({ kind: "before", provider: "zcode" });
    const next = moveQuotaProduct(order, selected, "kimi-code", target.kind === "before" ? target.provider : null);
    expect(quotaTabOrder(catalog, selected, next)).toEqual(["cursor", "codex", "kimi-code", "zcode", "grok", "claudeCode"]);
  });
});

describe("状态 column", () => {
  const product = (provider: RateLimitProvider, isDetected = true): QuotaProduct => ({
    provider,
    displayName: provider,
    availability: "ready",
    isDetected,
  });

  it("colors detection, missing configuration and read problems like macOS", () => {
    expect(quotaProductStatus(product("grok"), [])).toEqual({ kind: "ok", text: "已检测" });
    expect(quotaProductStatus(product("grok", false), [])).toEqual({ kind: "none", text: "未检测到" });
    expect(quotaProductStatus(product("zcode"), [], undefined, false, false)).toEqual({ kind: "warning", text: "需 Key" });
    expect(quotaProductStatus(product("zcode"), [], undefined, false, true)).toEqual({ kind: "ok", text: "已检测" });
    // Cursor ended the kept sign-in: said even while the card is off.
    expect(quotaProductStatus(product("cursor"), [], undefined, false, false, true)).toEqual({
      kind: "warning",
      text: "已失效",
    });
    const error = { provider: "grok" as const, status: { kind: "error" as const, message: "hidden" } };
    expect(quotaProductStatus(product("grok"), ["grok"], error)).toEqual({ kind: "error", text: "出错" });
    const paused = {
      provider: "grok" as const,
      status: { kind: "ok" as const },
      liveFailure: { code: "egressRegionUnsupported" as const, region: "CN" },
    };
    expect(quotaProductStatus(product("grok"), ["grok"], paused)).toEqual({ kind: "warning", text: "已暂停" });
  });
});

describe("关于", () => {
  it("tags external-test builds and links the configured site", () => {
    expect(settingsVersionText({ version: "0.7.10", updatesAvailable: true })).toBe("0.7.10");
    expect(settingsVersionText({ version: "0.7.10", updatesAvailable: false })).toBe("0.7.10 · 外测");
    expect(settingsVersionText(null)).toBe("");
    expect(siteUrl("https://vibefriends.com/api")).toBe("https://vibefriends.com");
    expect(siteUrl("http://localhost:3000")).toBe("http://localhost:3000");
    expect(siteUrl("file:///etc")).toBeNull();
    expect(siteUrl("")).toBeNull();
  });
});

// The pages drawn on the server against a fixture model: what each page
// holds, in which order, and what is gone.
describe("pages", () => {
  const products: QuotaProduct[] = (
    ["codex", "claudeCode", "kimi-code", "zcode", "grok", "cursor", "opencode-go", "antigravity"] as RateLimitProvider[]
  ).map((provider) => ({
    provider,
    displayName: { codex: "Codex", claudeCode: "Claude", "kimi-code": "Kimi Code", zcode: "ZCode", grok: "Grok", cursor: "Cursor", "opencode-go": "OpenCode Go", antigravity: "Antigravity / AGY" }[provider],
    availability: "ready",
    isDetected: true,
  }));
  const noop = () => {};

  function model(page: SettingsPage, overrides: Partial<SettingsModel> = {}): SettingsModel {
    return {
      status: {
        configured: true,
        apiUrl: "https://vibefriends.com",
        version: "0.7.10",
        isDev: false,
        runtimeAvailable: true,
        testDiagnosticsAvailable: false,
        updatesAvailable: true,
        apiKeyDisplay: "vbu_k3f9...a41c",
      },
      appName: "Vibe Usage",
      settings: {
        showCostInTray: true,
        showTokensInTray: false,
        codexRateLimitEnabled: true,
        claudeRateLimitEnabled: true,
        selectedQuotaProductIds: ["claudeCode", "codex"],
        quotaProductOrder: [],
        quotaSelectionInitialized: true,
        zCodeQuotaRegion: "bigModel",
        egressMode: "auto",
      },
      page,
      setPage: noop,
      autoStart: false,
      setAutoStart: noop,
      patchSettings: noop,
      syncState: { status: "idle", lastSyncAt: Date.now() - 26 * 60_000 },
      syncHistory: [],
      relinking: false,
      relinkUserCode: null,
      relinkError: null,
      startRelink: noop,
      cancelRelink: noop,
      askReset: noop,
      quotaProducts: products,
      quotaSnapshots: [],
      quotaBusy: false,
      quotaError: null,
      enabledOrder: ["claudeCode", "codex"],
      disabledOrder: ["kimi-code", "zcode", "grok", "cursor", "opencode-go", "antigravity"],
      quotaStatus: (product) => quotaProductStatus(product, ["claudeCode", "codex"]),
      toggleQuotaProduct: noop,
      rediscoverQuotaProducts: noop,
      moveQuotaProduct: noop,
      expanded: new Set(),
      toggleExpanded: noop,
      flash: null,
      egressMode: "auto",
      zCodeRegion: "bigModel",
      zCodeConfigured: false,
      zCodeDraft: "",
      setZCodeDraft: noop,
      zCodeReplacing: false,
      beginReplacingZCodeKey: noop,
      cancelReplacingZCodeKey: noop,
      zCodeError: null,
      setZCodeRegion: noop,
      saveZCodeKey: noop,
      removeZCodeKey: noop,
      cursorLogin: { state: "signedOut", supported: true },
      cursorLoginError: null,
      cursorLoginAction: noop,
      extraRoots: {},
      extraRootsBusy: false,
      extraRootsError: null,
      addExtraRoot: noop,
      removeExtraRoot: noop,
      updateOffer: null,
      updatePhase: { kind: "idle" },
      updateMessage: null,
      checkUpdate: noop,
      installUpdate: noop,
      diagnosticsBusy: false,
      diagnosticsText: null,
      diagnosticsCopied: false,
      diagnosticsMessage: null,
      copyDiagnostics: noop,
      testExportMessage: null,
      exportTestDiagnostics: noop,
      openExternal: noop,
      ...overrides,
    };
  }

  const render = (value: SettingsModel) =>
    renderToStaticMarkup(createElement(SettingsContext.Provider, { value }, createElement(SettingsWindow)));
  const text = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

  it("puts the five pages in the sidebar, with the red dot only after a failed sync", () => {
    const html = render(model("general"));
    expect(text(html)).toMatch(/通用 订阅配额 数据同步 数据目录 关于/);
    expect(html).not.toContain('aria-label="同步失败"');
    expect(render(model("general", { syncState: { status: "error", message: "失败" } }))).toContain(
      'aria-label="同步失败"',
    );
  });

  it("keeps the tray and startup switches on 通用", () => {
    const page = text(render(model("general")));
    expect(page).toContain("托盘显示费用");
    expect(page).toContain("托盘显示 Token");
    expect(page).toContain("开机自启动");
    expect(page).not.toContain("Dock");
  });

  it("lists 订阅配额 in the panel's order, the rest under 未开启, privacy below", () => {
    const page = text(render(model("quotas")));
    expect(page).toContain("2 / 8 已开启 · 拖动排序");
    expect(page).toContain("重新检测");
    expect(page).toMatch(/Claude .*Codex .*未开启 .*Kimi Code .*ZCode .*需 Key/);
    expect(page.indexOf("隐私保护")).toBeGreaterThan(page.indexOf("Antigravity / AGY"));
    expect(page).toContain("自动判断（推荐）");
    // macOS dropped the explanation (2026-11-20) and its stale Cursor line.
    expect(page).not.toContain("数据来源与检测");
    expect(page).not.toContain("等待官方配额接口");
  });

  it("opens ZCode's key and Cursor's sign-in under their rows", () => {
    const closed = render(model("quotas"));
    expect(closed).toContain('aria-label="展开 ZCode"');
    expect(closed).toContain('aria-label="展开 Cursor"');
    expect(closed).not.toContain("粘贴 API Key");
    const open = text(render(model("quotas", { expanded: new Set(["zcode", "cursor"]) })));
    expect(open).toContain("BigModel（国内）");
    expect(open).toContain("Z.ai（海外）");
    expect(open).toContain("粘贴 API Key");
    expect(open).toContain("未登录");
    const saved = text(render(model("quotas", { expanded: new Set(["zcode"]), zCodeConfigured: true })));
    expect(saved).toMatch(/Key 已保存 更换 移除/);
  });

  it("holds the key, the sync line with its bars, and the reset on 数据同步", () => {
    const records = Array.from({ length: 10 }, () => ({
      startedAt: 0,
      trigger: "scheduled" as const,
      outcome: "success" as const,
      durationMs: 1,
    }));
    const page = render(model("sync", { syncHistory: records }));
    expect(text(page)).toMatch(/API Key vbu_k3f9...a41c 重新链接/);
    expect(text(page)).toContain("正常 · 26 分钟前");
    expect(page).toContain('aria-label="最近 10 次同步，失败 0 次"');
    expect(text(page)).toContain("重置…");
    const unlinked = text(render(model("sync", { status: { ...model("sync").status!, configured: false, apiKeyDisplay: null } })));
    expect(unlinked).toMatch(/未配置 链接/);
    expect(unlinked).toContain("—");
  });

  it("lists the isolated directories on 数据目录", () => {
    const html = render(model("directories", { extraRoots: { codex: ["C:\\work\\.codex"] } }));
    const page = text(html);
    expect(page).toContain("高级");
    expect(page).toContain("隔离运行时目录");
    expect(page).toMatch(/Codex .*Grok .*Antigravity \/ AGY/);
    // Cut in the middle on screen; the whole path is the tooltip.
    expect(html).toContain('title="C:\\work\\.codex"');
  });

  it("shows identity, update, diagnostics and links on 关于", () => {
    const page = text(render(model("about")));
    expect(page).toContain("版本 0.7.10");
    expect(page).toContain("检查更新");
    expect(page).toContain("复制诊断信息");
    expect(page).not.toContain("导出诊断日志");
    expect(page).toMatch(/vibe-usage .*vibe-usage-windows .*VibeCafé/);
    expect(page).toContain("© 2026 VibeCafé · MIT 许可证");
    const test = text(
      render(model("about", { status: { ...model("about").status!, testDiagnosticsAvailable: true, updatesAvailable: false } })),
    );
    expect(test).toContain("导出诊断日志 测试版");
    expect(test).toContain("外测版不检查更新");
    expect(test).toContain("版本 0.7.10 · 外测");
  });
});
