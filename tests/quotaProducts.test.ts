import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  canonicalQuotaMeters,
  codexFiveHourPlaceholder,
  compactQuotaStatus,
  expectsCodexFiveHour,
  isZCodeConfigured,
  moveQuotaProduct,
  quotaEmptyStateText,
  quotaFailureText,
  quotaProductStatusText,
  quotaProvenanceText,
  quotaRetryableText,
  quotaTabOrder,
  quotaTabShowsWarning,
} from "../src/lib/quotaProducts";
import { ProviderRateLimit, QuotaProduct, RateLimitProvider } from "../src/lib/types";

describe("quota product presentation", () => {
  const grok: QuotaProduct = { provider: "grok", displayName: "Grok", availability: "ready", isDetected: true };

  it("distinguishes a discovered unread product from a known no-data result", () => {
    expect(quotaProductStatusText(grok)).toBe("已检测 · 未读取");
    expect(quotaProductStatusText(grok, undefined, "bigModel", { provider: "grok", status: { kind: "noData" } })).toBe("已检测 · 无数据");
  });

  it.each([
    ["ok", "读取成功"], ["disabled", "未启用"], ["unauthorized", "需重新登录"],
    ["retryableError", "读取失败 · 可重试"], ["error", "读取失败"],
  ] as const)("shows %s independently from discovery", (kind, label) => {
    const status = kind === "error" ? { kind, message: "must not be displayed" } : { kind };
    expect(quotaProductStatusText(grok, undefined, "bigModel", { provider: "grok", status })).toBe(`已检测 · ${label}`);
  });

  it("does not borrow another card's status", () => {
    expect(quotaProductStatusText(grok, undefined, "bigModel", { provider: "codex", status: { kind: "noData" } })).toBe("已检测 · 未读取");
  });

  it("separates regional credentials from discovery", () => {
    const product: QuotaProduct = { provider: "zcode", displayName: "ZCode", availability: "ready", isDetected: false };
    const credentials = { bigModelConfigured: true, zAiConfigured: false };
    expect(quotaProductStatusText(product, credentials, "zAI")).toBe("未检测到 · 需配置 API Key");
    expect(quotaProductStatusText(product, credentials, "bigModel")).toBe("未检测到 · API Key 已配置 · 未读取");
  });

  it("keeps pending protocol distinct from a disabled snapshot", () => {
    expect(quotaProductStatusText({ ...grok, provider: "cursor", availability: "pendingProtocol" }, undefined, "bigModel", { provider: "cursor", status: { kind: "disabled" } })).toBe("已检测 · 待接入");
  });

  it("describes discovery separately from protocol readiness", () => {
    const pending: QuotaProduct = {
      provider: "cursor",
      displayName: "Cursor",
      availability: "pendingProtocol",
      isDetected: true,
    };
    expect(quotaProductStatusText(pending)).toBe("已检测 · 待接入");
  });

  it("tracks ZCode regional credentials independently", () => {
    const status = { bigModelConfigured: true, zAiConfigured: false };
    expect(isZCodeConfigured(status, "bigModel")).toBe(true);
    expect(isZCodeConfigured(status, "zAI")).toBe(false);
  });

  it("shortens a settings row to the one fact it can act on", () => {
    expect(compactQuotaStatus("未检测到 · API Key 已配置 · 未读取")).toBe("已配置");
    expect(compactQuotaStatus("已检测 · 需配置 API Key")).toBe("待配置");
    expect(compactQuotaStatus("未检测到 · 需配置 API Key")).toBe("待配置");
    expect(compactQuotaStatus("已检测 · 待接入")).toBe("已检测 · 待接入");
  });
});

describe("quota meter layout", () => {
  it("puts generic periods first from shortest to longest", () => {
    const ordered = canonicalQuotaMeters([
      { id: "mcp", label: "MCP", utilization: 4, windowDuration: 30 * 86_400 },
      { id: "weekly", label: "Weekly", utilization: 30 },
      { id: "sonnet", label: "Sonnet", utilization: 40, windowDuration: 7 * 86_400 },
      { id: "five-hour", label: "5h", utilization: 10, windowDuration: 5 * 3_600 },
      { id: "extra", label: "额外", utilization: 50 },
    ]);

    expect(ordered.map((meter) => meter.label)).toEqual([
      "5h", "7d", "MCP", "Sonnet", "额外",
    ]);
    expect(ordered[1].windowDuration).toBe(7 * 86_400);
  });
});

describe("empty quota card copy", () => {
  const noData = (extra: Partial<ProviderRateLimit>): ProviderRateLimit => ({
    provider: "codex",
    status: { kind: "noData" },
    ...extra,
  });

  it("says a refresh is in flight instead of reporting a verdict", () => {
    expect(quotaEmptyStateText(noData({ emptyReason: "limitReached" }), false, true)).toBe(
      "正在读取订阅配额…",
    );
  });

  it("repeats the live source's own verdict when it reported one", () => {
    expect(quotaEmptyStateText(noData({ emptyReason: "limitReached" }), true)).toBe(
      "本期订阅配额已用满 · 等待额度重置",
    );
    expect(quotaEmptyStateText(noData({ emptyReason: "noWindow" }), true)).toBe(
      "当前没有生效的额度窗口",
    );
  });

  it("never borrows 「已用满」 for a source that cannot tell", () => {
    for (const snapshot of [noData({}), noData({ emptyReason: null })]) {
      expect(quotaEmptyStateText(snapshot, true)).toBe("暂未读取到订阅配额数据");
      expect(quotaEmptyStateText(snapshot, false)).toBe("未检测到本机安装或登录");
      expect(quotaEmptyStateText(snapshot, true)).not.toContain("已用满");
    }
  });

  it("separates a detected product from one that is not installed", () => {
    const product: ProviderRateLimit = { provider: "grok", status: { kind: "noData" } };
    expect(quotaEmptyStateText(product, true)).toBe("暂未读取到订阅配额数据");
    expect(quotaEmptyStateText(product, false)).toBe("未检测到本机安装或登录");
  });
});

describe("tab strip model", () => {
  const catalog: RateLimitProvider[] = ["codex", "claudeCode", "kimi-code", "zcode", "grok", "cursor", "opencode-go"];

  it("lists enabled products first, then the grey ones, each in stored order", () => {
    const stored: RateLimitProvider[] = ["opencode-go", "grok", "cursor", "codex", "claudeCode"];
    const order = quotaTabOrder(catalog, ["grok", "codex"], stored);

    expect(order.slice(0, 2)).toEqual(["grok", "codex"]);
    expect(new Set(order.slice(2))).toEqual(new Set(["opencode-go", "cursor", "claudeCode", "kimi-code", "zcode"]));
    // Every catalog product keeps a tab: an id the stored order forgot is
    // appended rather than dropped, and a ghost id disappears.
    expect(order).toHaveLength(catalog.length);
  });

  it("ignores ids that are no longer in the catalog", () => {
    const order = quotaTabOrder(catalog, ["codex"], ["codex", "ghost-product" as RateLimitProvider]);
    expect(order).not.toContain("ghost-product");
    expect(order[0]).toBe("codex");
  });

  it("keeps the stored order as render order after a drop", () => {
    const stored: RateLimitProvider[] = ["codex", "claudeCode", "grok", "kimi-code"];
    const enabled: RateLimitProvider[] = ["codex", "claudeCode", "grok"];

    expect(moveQuotaProduct(stored, enabled, "grok", "codex")).toEqual([
      "grok", "codex", "claudeCode", "kimi-code",
    ]);
  });

  it("drops past the last tab at the end of the dragged product's own group", () => {
    const stored: RateLimitProvider[] = ["codex", "claudeCode", "grok", "kimi-code"];
    const enabled: RateLimitProvider[] = ["codex", "claudeCode"];

    // grok is disabled, so "the end" is the end of the grey group, not of the row.
    expect(moveQuotaProduct(stored, enabled, "grok", null)).toEqual([
      "codex", "claudeCode", "kimi-code", "grok",
    ]);
  });

  it("normalizes a cross-group drop back into the dragged product's group", () => {
    const stored: RateLimitProvider[] = ["codex", "kimi-code"];
    const enabled: RateLimitProvider[] = ["codex"];

    // Dropping the grey kimi-code onto the enabled codex cannot lift it above it.
    expect(moveQuotaProduct(stored, enabled, "kimi-code", "codex")).toEqual(["codex", "kimi-code"]);
  });

  it("marks enabled products whose last read is not ok, and only those", () => {
    const enabled: RateLimitProvider[] = ["codex", "opencode-go"];
    const notEntitled: ProviderRateLimit = {
      provider: "opencode-go",
      status: { kind: "noData" },
      emptyReason: "notEntitled",
    };

    expect(quotaTabShowsWarning("opencode-go", enabled, notEntitled, false)).toBe(true);
    // A healthy read, a disabled product and a product with no snapshot yet stay plain.
    expect(quotaTabShowsWarning("codex", enabled, { provider: "codex", status: { kind: "ok" } }, false)).toBe(false);
    expect(quotaTabShowsWarning("grok", enabled, notEntitled, false)).toBe(false);
    expect(quotaTabShowsWarning("codex", enabled, undefined, false)).toBe(false);
    // A fetch in flight is not a problem: the card shows its spinner.
    expect(quotaTabShowsWarning("opencode-go", enabled, notEntitled, true)).toBe(false);
  });
});

describe("empty state reasons", () => {
  it("names the product for a missing subscription", () => {
    expect(
      quotaEmptyStateText({ provider: "opencode-go", status: { kind: "noData" }, emptyReason: "notEntitled" },
        true, false, "OpenCode Go"),
    ).toBe("未订阅 OpenCode Go");
  });

  it("explains a Claude session that has no plan windows", () => {
    expect(
      quotaEmptyStateText({ provider: "claudeCode", status: { kind: "noData" }, emptyReason: "sessionWithoutPlanLimits" },
        true),
    ).toBe("当前登录方式不含订阅额度（API Key / Bedrock / Vertex）");
  });
});

describe("read failure reasons and sources", () => {
  const claude = (extra: Partial<ProviderRateLimit>): ProviderRateLimit => ({
    provider: "claudeCode",
    status: { kind: "noData" },
    ...extra,
  });

  it("states the Claude probe's own reason instead of a neutral empty line", () => {
    const missing = claude({ liveFailure: { code: "claudeBinaryNotFound" } });
    // Detected by a leftover ~/.claude but no program: the read says why.
    expect(quotaEmptyStateText(missing, true, false, "Claude")).toBe("未找到 Claude Code 程序");
    expect(quotaEmptyStateText(missing, true, false, "Claude")).not.toBe("暂未读取到订阅配额数据");
    expect(quotaProvenanceText(missing, "Claude")).toBeNull();
  });

  it("names the failure and the channel on a retryable card", () => {
    const timedOut = claude({
      status: { kind: "retryableError" },
      source: "live",
      liveFailure: { code: "claudeTimedOut" },
    });
    expect(quotaRetryableText(timedOut, "Claude")).toBe("Claude 配额读取超时");
    expect(quotaProvenanceText(timedOut, "Claude")).toBe("来源：实时读取");
    // A retryable card from an older backend keeps the old wording.
    expect(quotaRetryableText({ provider: "grok", status: { kind: "retryableError" } }, "Grok")).toBe(
      "暂时无法读取订阅配额",
    );
  });

  it("says where shown numbers came from only after the live read failed", () => {
    const cached = claude({
      status: { kind: "ok" },
      source: "cache",
      liveFailure: { code: "claudeNoResponse" },
    });
    expect(quotaProvenanceText(cached, "Claude")).toBe("来源：缓存 · Claude 没有响应配额请求");
    // A successful live card stays quiet, as on macOS.
    expect(quotaProvenanceText(claude({ status: { kind: "ok" }, source: "live" }), "Claude")).toBeNull();
    // The CLI answering from its cache.
    expect(
      quotaProvenanceText(
        { provider: "grok", status: { kind: "ok" }, source: "cache", liveFailure: { code: "cliReportedFailure" } },
        "Grok",
      ),
    ).toBe("来源：缓存 · 实时读取失败");
    // Codex session JSONL after the endpoint failed.
    expect(
      quotaProvenanceText(
        {
          provider: "codex",
          status: { kind: "ok" },
          source: "localLog",
          liveFailure: { code: "endpointHttpStatus", status: 502 },
        },
        "Codex",
      ),
    ).toBe("来源：本地日志 · Codex 配额接口返回错误 502");
  });

  it("splits the Claude Desktop record into its specific reasons", () => {
    const label = "Claude 桌面额度记录 · 非本机独占用量";
    const cases: Array<[string, string]> = [
      ["desktopRecordStale", "Claude 桌面端记录已过期"],
      ["desktopMultipleInstalls", "装了多份 Claude 桌面端，账号不明确"],
      ["desktopMixedOrganizations", "Claude 桌面端记录涉及多个组织，账号不明确"],
      ["desktopRecordUnreadable", "Claude 桌面端记录无法解析"],
    ];
    for (const [code, text] of cases) {
      const snapshot = claude({
        sourceLabel: label,
        source: "desktopRecord",
        liveFailure: { code: code as "desktopRecordStale" },
      });
      expect(quotaEmptyStateText(snapshot, true, false, "Claude")).toBe(text);
      expect(quotaProvenanceText(snapshot, "Claude")).toBe(label);
    }
    // An older backend's Desktop snapshot without a reason keeps the old line.
    expect(quotaEmptyStateText(claude({ sourceLabel: label }), true)).toBe(
      "桌面额度记录缺失、过期或账号不明确",
    );
  });

  it("lets a definitive emptyReason outrank any failure annotation", () => {
    expect(
      quotaEmptyStateText(
        claude({ emptyReason: "sessionWithoutPlanLimits", liveFailure: { code: "claudeNoWindows" } }),
        true,
      ),
    ).toBe("当前登录方式不含订阅额度（API Key / Bedrock / Vertex）");
  });

  it("uses the product name for endpoint and credential reasons", () => {
    expect(quotaFailureText({ code: "credentialsNotFound" }, "Codex")).toBe("未找到 Codex 登录信息");
    expect(quotaFailureText({ code: "endpointTimedOut" }, "Codex")).toBe("Codex 配额接口请求超时");
    expect(quotaFailureText({ code: "endpointUnreachable" }, "Codex")).toBe("Codex 配额接口连接失败");
    expect(quotaFailureText({ code: "cliTimedOut" }, "Kimi Code")).toBe("配额 CLI 读取超时");
    expect(quotaFailureText({ code: "credentialStoreUnavailable" }, "ZCode")).toBe(
      "无法读取 Windows 凭据管理器",
    );
  });

  it("never echoes anything but the fixed wording", () => {
    const unknown = { code: "somethingNew" } as unknown as { code: "cliFailed" };
    expect(quotaFailureText(unknown, "Grok")).toBe("暂时无法读取订阅配额");
  });

  it("keeps the re-login card free of extra lines", () => {
    expect(
      quotaProvenanceText(
        { provider: "codex", status: { kind: "unauthorized" }, source: "live", liveFailure: { code: "credentialsRejected" } },
        "Codex",
      ),
    ).toBeNull();
  });
});

describe("Codex 5h window expectation", () => {
  const codex = (planLabel?: string): ProviderRateLimit => ({
    provider: "codex",
    status: { kind: "ok" },
    planLabel,
  });

  it.each(["Plus", "Pro", "Prolite", "Business", "Business ProLite"])(
    "a paid plan (%s) explains a missing 5h window, as on macOS",
    (plan) => {
      expect(expectsCodexFiveHour(codex(plan))).toBe(true);
    },
  );

  it("free, unknown and non-Codex cards do not", () => {
    expect(expectsCodexFiveHour(codex("Free"))).toBe(false);
    expect(expectsCodexFiveHour(codex())).toBe(false);
    expect(expectsCodexFiveHour(codex("self_serve_business_prolite"))).toBe(false);
    expect(
      expectsCodexFiveHour({ provider: "claudeCode", status: { kind: "ok" }, planLabel: "Pro" }),
    ).toBe(false);
  });
});

describe("Codex 5h slot wording (macOS rule, 2026-10-24)", () => {
  const codex = (planLabel: string, extra: Partial<ProviderRateLimit> = {}): ProviderRateLimit => ({
    provider: "codex",
    status: { kind: "ok" },
    planLabel,
    sevenDay: { utilization: 10 },
    ...extra,
  });

  it("the live endpoint without a 5h window means the limit is off", () => {
    for (const plan of ["Plus", "Business", "Business ProLite", "Pro"]) {
      expect(codexFiveHourPlaceholder(codex(plan, { fiveHourNotEnforced: true }))).toBe("不限额");
    }
  });

  it("Pro and Prolite read 不限额 even from the session log", () => {
    expect(codexFiveHourPlaceholder(codex("Pro"))).toBe("不限额");
    expect(codexFiveHourPlaceholder(codex("Prolite"))).toBe("不限额");
  });

  it("other paid plans from the session log only say there was no recent activity", () => {
    for (const plan of ["Plus", "Business", "Business ProLite"]) {
      expect(codexFiveHourPlaceholder(codex(plan))).toBe("近 5 小时无活动");
    }
  });

  it("no slot when a 5h window exists, or the plan has none", () => {
    expect(
      codexFiveHourPlaceholder(codex("Pro", { fiveHour: { utilization: 3 }, fiveHourNotEnforced: true })),
    ).toBeNull();
    expect(codexFiveHourPlaceholder(codex("Free", { fiveHourNotEnforced: true }))).toBeNull();
    expect(
      codexFiveHourPlaceholder({ provider: "claudeCode", status: { kind: "ok" }, planLabel: "Pro" }),
    ).toBeNull();
  });

  it("the old 官方当前未启用 wording is gone from the card", () => {
    const card = readFileSync("src/components/RateLimitCard.tsx", "utf-8");
    expect(card).not.toContain("官方当前未启用");
    expect(card).toContain("codexFiveHourPlaceholder(snapshot)");
  });
});
