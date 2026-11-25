import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  canonicalQuotaMeters,
  codexFiveHourPlaceholder,
  quotaAuthorizationText,
  expectsCodexFiveHour,
  isZCodeConfigured,
  moveQuotaProduct,
  quotaEmptyStateText,
  quotaFailureText,
  quotaProductNote,
  quotaProductStatusText,
  quotaProvenanceText,
  quotaRetryableText,
  quotaTabOrder,
  quotaTabShowsWarning,
  quotaTooltipLines,
  quotaTooltipTitle,
} from "../src/lib/quotaProducts";
import { ProviderRateLimit, QuotaProduct, RateLimitProvider } from "../src/lib/types";

describe("quota product presentation", () => {
  const grok: QuotaProduct = { provider: "grok", displayName: "Grok", availability: "ready", isDetected: true };

  const enabled = ["grok"] as RateLimitProvider[];
  const result = (extra: Partial<ProviderRateLimit>): ProviderRateLimit => ({
    provider: "grok", status: { kind: "noData" }, ...extra,
  });

  it("distinguishes detection, an unread placeholder and a settled empty answer", () => {
    expect(quotaProductStatusText(grok, enabled)).toBe("已检测");
    expect(quotaProductStatusText({ ...grok, isDetected: false }, enabled)).toBe("未检测到");
    expect(quotaProductStatusText(grok, enabled, result({}))).toBe("未读取");
    expect(quotaProductStatusText(grok, enabled, result({ fetchedAt: 1 }))).toBe("无数据");
  });

  it.each([
    ["ok", "已检测"], ["disabled", "未启用"], ["unauthorized", "已失效"],
    ["retryableError", "失败"], ["error", "出错"],
  ] as const)("describes a settled %s", (kind, label) => {
    const status = kind === "error" ? { kind, message: "must not be displayed" } : { kind };
    expect(quotaProductStatusText(grok, enabled, result({ status }))).toBe(label);
  });

  it("shares the tab's enabled and refreshing gates and ignores other products", () => {
    const failed = result({ liveFailure: { code: "programNotFound" } });
    expect(quotaProductStatusText(grok, enabled, failed)).toBe("未安装");
    expect(quotaProductStatusText(grok, [], failed)).toBe("已检测");
    expect(quotaProductStatusText(grok, enabled, failed, true)).toBe("已检测");
    expect(quotaProductStatusText(grok, enabled, { ...failed, provider: "codex" })).toBe("已检测");
    expect(quotaProductStatusText(grok, enabled, { ...failed, status: { kind: "ok" } })).toBe("已检测");
  });

  it("keeps regional Key configuration unless there is a read warning", () => {
    const product = { ...grok, provider: "zcode" } as const;
    for (const configured of [false, true]) {
      expect(quotaProductStatusText(product, [], undefined, false, configured)).toBe(configured ? "已配置" : "待配置");
      for (const [code, label] of [["credentialsNotFound", "需 Key"], ["credentialsRejected", "Key 无效"]] as const) {
        const snapshot = result({ provider: "zcode", status: { kind: "unauthorized" }, liveFailure: { code } });
        expect(quotaProductStatusText(product, ["zcode"], snapshot, false, configured)).toBe(label);
      }
    }
    const keys = { bigModelConfigured: true, zAiConfigured: false };
    expect(isZCodeConfigured(keys, "bigModel")).toBe(true);
    expect(isZCodeConfigured(keys, "zAI")).toBe(false);
  });

  it("keeps Cursor's pending wording even with a disabled snapshot", () => {
    const product = { ...grok, provider: "cursor", availability: "pendingProtocol" } as const;
    expect(quotaProductStatusText(product, ["cursor"], result({ provider: "cursor", status: { kind: "disabled" } }))).toBe("已检测 · 待接入");
    expect(quotaProductStatusText({ ...product, isDetected: false }, [])).toBe("待接入");
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
      "额度已用满 · 等待重置",
    );
    expect(quotaEmptyStateText(noData({ emptyReason: "noWindow" }), true)).toBe(
      "没有生效的额度窗口",
    );
  });

  it("never borrows 「已用满」 for a source that cannot tell", () => {
    for (const snapshot of [noData({}), noData({ emptyReason: null })]) {
      expect(quotaEmptyStateText(snapshot, true)).toBe("尚未读取");
      expect(quotaEmptyStateText(snapshot, false)).toBe("尚未读取");
      expect(quotaEmptyStateText(snapshot, true)).not.toContain("已用满");
    }
  });

  it("separates a detected product from one that is not installed", () => {
    const product: ProviderRateLimit = { provider: "grok", fetchedAt: 1, status: { kind: "noData" } };
    expect(quotaEmptyStateText(product, true)).toBe("没读到额度，原因未知");
    expect(quotaEmptyStateText(product, false)).toBe("未检测到安装");
  });
});

describe("tab strip model", () => {
  const catalog: RateLimitProvider[] = [
    "codex", "claudeCode", "kimi-code", "zcode", "grok", "cursor", "opencode-go", "antigravity",
  ];

  it("lists enabled products first, then the grey ones, each in stored order", () => {
    const stored: RateLimitProvider[] = ["opencode-go", "grok", "cursor", "codex", "claudeCode"];
    const order = quotaTabOrder(catalog, ["grok", "codex"], stored);

    expect(order.slice(0, 2)).toEqual(["grok", "codex"]);
    expect(new Set(order.slice(2))).toEqual(
      new Set(["opencode-go", "cursor", "claudeCode", "kimi-code", "zcode", "antigravity"]),
    );
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

describe("OpenCode Go login states", () => {
  const snapshot = (extra: Partial<ProviderRateLimit>): ProviderRateLimit => ({
    provider: "opencode-go",
    status: { kind: "noData" },
    ...extra,
  });

  it("tells where to log in instead of saying the login is missing", () => {
    const missing = snapshot({ liveFailure: { code: "credentialsNotFound" } });
    expect(quotaEmptyStateText(missing, true, false, "OpenCode Go")).toBe("在 OpenCode 里登录后自动显示");
  });

  it("asks to open OpenCode when its account sign-in has expired", () => {
    const expired = snapshot({ liveFailure: { code: "credentialsExpired" } });
    expect(quotaEmptyStateText(expired, true, false, "OpenCode Go")).toBe("打开 OpenCode 后自动显示");
  });

  it("leaves a subscription answer and other products' wording alone", () => {
    const notEntitled = snapshot({ emptyReason: "notEntitled", liveFailure: { code: "credentialsNotFound" } });
    expect(quotaEmptyStateText(notEntitled, true, false, "OpenCode Go")).toBe("未订阅 OpenCode Go");
    expect(quotaFailureText({ code: "credentialsNotFound" }, "codex")).toBe("未找到登录信息");
    expect(quotaFailureText({ code: "credentialsExpired" }, "kimi-code")).toBe("登录已失效");
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
    ).toBe("当前登录方式不含订阅额度");
  });

  it("names the organization's Claude Code policy instead of reading like a failure", () => {
    const off: ProviderRateLimit = {
      provider: "claudeCode",
      status: { kind: "noData" },
      emptyReason: "nonessentialTrafficDisabled",
    };
    expect(quotaEmptyStateText(off, true)).toBe("组织策略关闭了 Claude Code 非必要流量 · 无法读取");
    // The policy is not about installation, so detection does not change it.
    expect(quotaEmptyStateText(off, false)).toBe("组织策略关闭了 Claude Code 非必要流量 · 无法读取");
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
    expect(quotaEmptyStateText(missing, true, false, "Claude")).toBe("未找到 Claude Code");
    expect(quotaEmptyStateText(missing, true, false, "Claude")).not.toBe("尚未读取");
    expect(quotaProvenanceText(missing)).toBeNull();
  });

  it("names the failure and the channel on a retryable card", () => {
    const timedOut = claude({
      status: { kind: "retryableError" },
      source: "live",
      liveFailure: { code: "claudeTimedOut" },
    });
    expect(quotaRetryableText(timedOut)).toBe("读取超时");
    expect(quotaProvenanceText(timedOut)).toBe("来源：实时读取");
    expect(quotaProvenanceText({ ...timedOut, source: "localLog" })).toBe("来源：本地日志");
    expect(quotaProvenanceText({ ...timedOut, source: "cache" })).toBe("来源：缓存");
    expect(quotaRetryableText({ provider: "grok", status: { kind: "retryableError" } })).toBe(
      "暂时读不到额度",
    );
  });

  it("says where shown numbers came from only after the live read failed", () => {
    const cached = claude({
      status: { kind: "ok" },
      source: "cache",
      liveFailure: { code: "claudeNoResponse" },
    });
    expect(quotaProvenanceText(cached)).toBe("来源：缓存 · 没有响应");
    // A successful live card stays quiet, as on macOS.
    expect(quotaProvenanceText(claude({ status: { kind: "ok" }, source: "live" }))).toBeNull();
    // The CLI answering from its cache.
    expect(
      quotaProvenanceText(
        { provider: "grok", status: { kind: "ok" }, source: "cache", liveFailure: { code: "cliReportedFailure" } },
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
      ),
    ).toBe("来源：本地日志 · 接口报错 502");
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
      expect(quotaProvenanceText(snapshot)).toBe(label);
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
    ).toBe("当前登录方式不含订阅额度");
  });

  it("keeps endpoint and credential reasons short", () => {
    expect(quotaFailureText({ code: "credentialsNotFound" }, "codex")).toBe("未找到登录信息");
    expect(quotaFailureText({ code: "endpointTimedOut" }, "codex")).toBe("读取超时");
    expect(quotaFailureText({ code: "endpointUnreachable" }, "codex")).toBe("连接失败");
    expect(quotaFailureText({ code: "cliTimedOut" }, "kimi-code")).toBe("读取超时");
    expect(quotaFailureText({ code: "credentialStoreUnavailable" }, "zcode")).toBe(
      "无法读取 Windows 凭据管理器",
    );
  });

  it("says why a read was paused in one line, with no retry", () => {
    expect(quotaFailureText({ code: "egressRegionUnsupported", region: "CN" }, "codex")).toBe(
      "网络出口在中国大陆，已暂停读取",
    );
    expect(quotaFailureText({ code: "egressRegionUnsupported", region: "HK" }, "claudeCode")).toBe(
      "网络出口在香港，已暂停读取",
    );
    expect(quotaFailureText({ code: "egressRegionUnsupported", region: "MO" }, "claudeCode")).toBe(
      "网络出口在澳门，已暂停读取",
    );
    expect(quotaFailureText({ code: "egressRegionUnsupported", region: "Other" }, "claudeCode")).toBe(
      "网络出口不在支持地区，已暂停读取",
    );
    expect(quotaFailureText({ code: "egressUnconfirmed" }, "codex")).toBe("无法确认网络出口，已暂停读取");
    expect(quotaFailureText({ code: "proxyNotDetected" }, "codex")).toBe("未检测到代理，已暂停读取");
    const paused = { provider: "codex", status: { kind: "noData" }, liveFailure: { code: "egressUnconfirmed" } } as const;
    expect(quotaEmptyStateText(paused, true)).toBe("无法确认网络出口，已暂停读取");
    expect(
      quotaProvenanceText(
        {
          provider: "codex",
          status: { kind: "ok" },
          source: "localLog",
          liveFailure: { code: "egressRegionUnsupported", region: "CN" },
        },
      ),
    ).toBe("来源：本地日志 · 网络出口在中国大陆，已暂停读取");
    // The tab warns while those numbers are kept.
    expect(
      quotaTabShowsWarning(
        "codex",
        ["codex"],
        { provider: "codex", status: { kind: "ok" }, liveFailure: { code: "egressUnconfirmed" } },
        false,
      ),
    ).toBe(true);
    expect(
      quotaTabShowsWarning("codex", ["codex"], { provider: "codex", status: { kind: "ok" } }, false),
    ).toBe(false);
  });

  it("never echoes anything but the fixed wording", () => {
    const unknown = { code: "somethingNew" } as unknown as { code: "cliFailed" };
    expect(quotaFailureText(unknown, "grok")).toBe("暂时读不到额度");
  });

  it("keeps the re-login card free of extra lines", () => {
    expect(
      quotaProvenanceText(
        { provider: "codex", status: { kind: "unauthorized" }, source: "live", liveFailure: { code: "credentialsRejected" } },
      ),
    ).toBeNull();
  });
});

describe("quota status copy shared with macOS", () => {
  it.each([
    ["kimi-code", "credentialsNotFound", "请先登录 Kimi Code", "需登录"],
    ["cursor", "credentialsNotFound", "请先登录 Cursor", "需登录"],
    ["grok", "credentialsNotFound", "请先登录", "需登录"],
    ["kimi-code", "credentialsRejected", "登录已失效，请重新登录", "已失效"],
    ["codex", "credentialsRejected", "登录已失效，请重新登录", "已失效"],
    ["opencode-go", "credentialsRejected", "请在 OpenCode 中重新登录后重试", "已失效"],
    ["antigravity", "programNotFound", "请安装 agy 或 Antigravity", "未安装"],
    ["antigravity", "credentialsRejected", "请在 Antigravity 中登录", "需登录"],
    ["zcode", "credentialsNotFound", "请在设置中配置 Z.ai API Key", "需 Key"],
    ["zcode", "credentialsRejected", "API Key 无效，请在设置中更换", "Key 无效"],
  ] as const)("maps %s login state %s", (provider, code, card, short) => {
    const snapshot = { provider, liveFailure: { code }, status: { kind: "unauthorized" } } as const;
    expect(quotaAuthorizationText(snapshot, "zAI")).toBe(card);
    expect(quotaProductStatusText({ provider, isDetected: true, displayName: provider, availability: "ready" },
      [provider], snapshot)).toBe(short);
    if (provider === "zcode" && code === "credentialsNotFound") {
      expect(quotaAuthorizationText(snapshot, "bigModel")).toBe("请在设置中配置 BigModel API Key");
    }
  });

  it.each([
    ["grok", "periodEnded", "账期已结束 · 用一次 Grok 后更新", "待更新"],
    ["antigravity", "periodEnded", "额度已重置 · 下次读取时更新", "待更新"],
    ["codex", "periodEnded", "上个周期已结束", "待更新"],
    ["grok", "noLocalRecord", "暂无记录 · 用一次 Grok 后更新", "无记录"],
    ["antigravity", "noLocalRecord", "用过 Antigravity 后显示", "无记录"],
    ["codex", "noLocalRecord", "本机暂无记录", "无记录"],
    ["codex", "noWindow", "没有生效的额度窗口", "无窗口"],
    ["codex", "limitReached", "额度已用满 · 等待重置", "已用满"],
    ["claudeCode", "sessionWithoutPlanLimits", "当前登录方式不含订阅额度", "不适用"],
    ["claudeCode", "nonessentialTrafficDisabled", "组织策略关闭了 Claude Code 非必要流量 · 无法读取", "已禁止"],
    ["opencode-go", "notEntitled", "未订阅 OpenCode Go", "未订阅"],
  ] as const)("maps %s empty result %s ahead of a live failure", (provider, emptyReason, card, short) => {
    const snapshot = { provider, emptyReason, liveFailure: { code: "cliFailed" }, status: { kind: "noData" } } as const;
    expect(quotaEmptyStateText(snapshot, false, false, "OpenCode Go")).toBe(card);
    expect(quotaProductStatusText({ provider, isDetected: false, displayName: provider, availability: "ready" },
      [provider], snapshot)).toBe(short);
  });

  it.each([
    ["credentialsNotFound", "运行 grok login 后可实时读取", "需登录"],
    ["credentialsRejected", "登录已失效", "已失效"],
    ["credentialsUnreadable", "读不到登录信息", "读不了"],
    ["credentialsEncrypted", "登录信息已加密，暂时读不了", "不支持"],
    ["loginRefreshFailed", "登录续期失败", "续期失败"],
    ["programNotFound", "未找到 Grok 命令行", "未安装"],
    ["programOutdated", "版本较旧，升级后可实时读取", "需升级"],
    ["programNoAnswer", "没有返回额度", "无响应"],
    ["localRecordUnreadable", "读不了本机记录", "读不了"],
    ["endpointTimedOut", "读取超时", "超时"],
    ["endpointUnreachable", "连接失败", "连不上"],
    ["endpointHttpStatus", "接口报错", "接口报错"],
    ["endpointUnrecognized", "返回数据无法识别", "数据异常"],
    ["cliFailed", "配额 CLI 出错", "CLI 出错"],
    ["cliReportedFailure", "实时读取失败", "失败"],
    ["cliSchemaMismatch", "配额 CLI 版本不兼容", "CLI 出错"],
    ["cliRuntimeMissing", "缺少 Node.js", "缺 Node"],
    ["cliNoResult", "配额 CLI 没有返回结果", "CLI 出错"],
    ["cliUnrecognizedOutput", "配额 CLI 输出无法识别", "数据异常"],
    ["claudeBinaryNotFound", "未找到 Claude Code", "未安装"],
    ["claudeLaunchFailed", "启动失败", "启动失败"],
    ["claudeNoResponse", "没有响应", "无响应"],
    ["desktopRecordStale", "Claude 桌面端记录已过期", "已过期"],
    ["desktopMixedOrganizations", "Claude 桌面端记录涉及多个组织，账号不明确", "不明确"],
    ["desktopMultipleInstalls", "装了多份 Claude 桌面端，账号不明确", "不明确"],
    ["desktopRecordUnreadable", "Claude 桌面端记录无法解析", "数据异常"],
    ["credentialStoreUnavailable", "无法读取 Windows 凭据管理器", "读不了"],
  ] as const)("maps %s on a failed read and on fallback numbers", (code, card, short) => {
    const product = { provider: "grok", isDetected: true, displayName: "Grok", availability: "ready" } as const;
    const snapshot = { provider: product.provider, liveFailure: { code }, status: { kind: "retryableError" } } as const;
    expect(quotaRetryableText(snapshot)).toBe(card);
    expect(quotaProductStatusText(product, ["grok"], snapshot)).toBe(short);
    for (const [source, label] of [["localLog", "本地日志"], ["cache", "缓存"]] as const) {
      const fallback = { ...snapshot, source, status: { kind: "ok" } } as const;
      expect(quotaProvenanceText(fallback)).toBe(`来源：${label} · ${card}`);
      expect(quotaProductStatusText(product, ["grok"], fallback)).toBe("已检测");
    }
  });

  it.each(["proxyUnsupported", "proxyNotDetected", "egressUnconfirmed", "egressRegionUnsupported"] as const)(
    "shows a paused row and tab even with local numbers for %s", (code) => {
      const product = { provider: "grok", isDetected: true, displayName: "Grok", availability: "ready" } as const;
      const snapshot = { provider: product.provider, liveFailure: { code }, source: "localLog", status: { kind: "ok" } } as const;
      expect(quotaTabShowsWarning("grok", ["grok"], snapshot, false)).toBe(true);
      expect(quotaProductStatusText(product, ["grok"], snapshot)).toBe("已暂停");
      if (code === "proxyUnsupported") {
        expect(quotaProvenanceText(snapshot)).toBe("来源：本地日志 · 代理无法照着走，已暂停读取");
      }
    },
  );
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

describe("settings product notes", () => {
  it("adds Google's own statement under Antigravity only, as on macOS", () => {
    expect(quotaProductNote("antigravity")).toBe("Google 表示不支持第三方读取额度");
    for (const provider of ["codex", "claudeCode", "kimi-code", "zcode", "grok", "opencode-go", "cursor"] as RateLimitProvider[]) {
      expect(quotaProductNote(provider)).toBeNull();
    }
  });
});

describe("quota tooltip (macOS TooltipView)", () => {
  const now = new Date("2026-11-25T00:00:00Z");
  const epoch = (iso: string) => new Date(iso).getTime() / 1000;

  it("spells out the common windows and keeps any other label", () => {
    expect(quotaTooltipTitle("5h")).toBe("5 小时窗口");
    expect(quotaTooltipTitle("7d")).toBe("7 天窗口");
    expect(quotaTooltipTitle("Month")).toBe("月度窗口");
    expect(quotaTooltipTitle("额外")).toBe("额外");
  });

  it("puts Cursor's split under the Month row's usage and time", () => {
    const lines = quotaTooltipLines(
      { utilization: 48, resetsAt: epoch("2026-12-01T00:00:00Z"), windowDuration: 30 * 86400 },
      [
        { label: "Cursor 模型", utilization: 4 },
        { label: "第三方模型", utilization: 88 },
      ],
      now,
    );
    expect(lines.map(({ kind, label, value }) => ({ kind, label, value }))).toEqual([
      { kind: "usage", label: "Token 用量", value: "已使用 48%" },
      { kind: "time", label: "时间", value: "已过去 80% · 剩余 6d" },
      { kind: "part", label: "Cursor 模型", value: "4%" },
      { kind: "part", label: "第三方模型", value: "88%" },
    ]);
  });

  it("falls back to the reset countdown, then to 未知", () => {
    const resetOnly = quotaTooltipLines({ utilization: 25, resetsAt: epoch("2026-11-25T02:30:00Z") }, [], now);
    expect(resetOnly[1]).toEqual({ kind: "time", label: "重置", value: "剩余 2h 30m" });
    const unknown = quotaTooltipLines({ utilization: 25 }, undefined, now);
    expect(unknown).toHaveLength(2);
    expect(unknown[1]).toEqual({ kind: "time", label: "时间", value: "未知", muted: true });
  });
});
