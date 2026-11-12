import {
  ProviderRateLimit,
  QuotaProduct,
  RateLimitLiveFailure,
  RateLimitMeter,
  RateLimitProvider,
  RateLimitSource,
  ZCodeCredentialStatus,
  ZCodeQuotaRegion,
} from "./types";

type PeriodPresentation = {
  label: string;
  seconds: number;
  inferredWindowDuration?: number;
};

function periodPresentation(meter: RateLimitMeter): PeriodPresentation | null {
  const compact = meter.label.trim().toLowerCase().replace(/\s+/g, "");
  const day = 24 * 60 * 60;
  const aliases: Record<string, PeriodPresentation> = {
    daily: { label: "1d", seconds: day, inferredWindowDuration: day },
    day: { label: "1d", seconds: day, inferredWindowDuration: day },
    weekly: { label: "7d", seconds: 7 * day, inferredWindowDuration: 7 * day },
    week: { label: "7d", seconds: 7 * day, inferredWindowDuration: 7 * day },
    monthly: { label: "Month", seconds: 30 * day },
    month: { label: "Month", seconds: 30 * day },
  };
  const alias = aliases[compact];
  if (alias) {
    return { ...alias, seconds: meter.windowDuration ?? alias.seconds };
  }

  const match = /^(\d+(?:\.\d+)?)(m|h|d|w)$/.exec(compact);
  if (!match) return null;
  const unit = match[2] as "m" | "h" | "d" | "w";
  const multiplier: Record<typeof unit, number> = {
    m: 60,
    h: 3600,
    d: day,
    w: 7 * day,
  };
  const seconds = meter.windowDuration ?? Number(match[1]) * multiplier[unit];
  return {
    label: compact === "1w" ? "7d" : compact,
    seconds,
    inferredWindowDuration: seconds,
  };
}

/** Generic time windows lead from shortest to longest. Provider-specific
 * meters keep their original relative order after those common windows. */
export function canonicalQuotaMeters(meters: RateLimitMeter[]): RateLimitMeter[] {
  return meters
    .map((original, index) => {
      const meter = { ...original };
      const period = periodPresentation(meter);
      if (period) {
        meter.label = period.label;
        if (meter.windowDuration == null && period.inferredWindowDuration != null) {
          meter.windowDuration = period.inferredWindowDuration;
        }
      }
      return { meter, periodSeconds: period?.seconds, index };
    })
    .sort((left, right) => {
      const leftIsPeriod = left.periodSeconds != null;
      const rightIsPeriod = right.periodSeconds != null;
      if (leftIsPeriod !== rightIsPeriod) return leftIsPeriod ? -1 : 1;
      if (left.periodSeconds != null && right.periodSeconds != null &&
          left.periodSeconds !== right.periodSeconds) {
        return left.periodSeconds - right.periodSeconds;
      }
      return left.index - right.index;
    })
    .map(({ meter }) => meter);
}

export function providerLabel(provider: RateLimitProvider, products: QuotaProduct[]): string {
  return products.find((product) => product.provider === provider)?.displayName ?? provider;
}

export function isZCodeConfigured(
  status: ZCodeCredentialStatus,
  region: ZCodeQuotaRegion,
): boolean {
  return region === "bigModel" ? status.bigModelConfigured : status.zAiConfigured;
}

export function quotaProductStatusText(
  product: QuotaProduct,
  zCodeStatus?: ZCodeCredentialStatus,
  zCodeRegion: ZCodeQuotaRegion = "bigModel",
  snapshot?: ProviderRateLimit,
): string {
  const detected = product.isDetected ? "已检测" : "未检测到";
  if (product.provider === "zcode" && product.availability === "ready" && zCodeStatus) {
    const configured = isZCodeConfigured(zCodeStatus, zCodeRegion);
    if (!configured) return `${detected} · 需配置 API Key`;
  }
  if (product.availability === "pendingProtocol") {
    return product.isDetected ? "已检测 · 待接入" : "待接入";
  }
  // Never-requested products and another provider's snapshot are not no-data.
  const result = snapshot?.provider === product.provider ? snapshot : undefined;
  const reading = result ? {
    ok: "读取成功",
    noData: "无数据",
    disabled: "未启用",
    unauthorized: "需重新登录",
    retryableError: "读取失败 · 可重试",
    error: "读取失败",
  }[result.status.kind] : "未读取";
  const configured = product.provider === "zcode" && zCodeStatus && isZCodeConfigured(zCodeStatus, zCodeRegion);
  return `${detected}${configured ? " · API Key 已配置" : ""} · ${reading}`;
}

/**
 * Settings rows show the one fact the row can act on instead of the full
 * selector status line. Mirrors SettingsView.compactQuotaStatus: wording this
 * map does not recognize passes through untouched instead of being invented.
 */
export function compactQuotaStatus(status: string): string {
  if (status.includes("已配置")) return "已配置";
  if (status.includes("需配置") || status.includes("未检测到")) return "待配置";
  return status;
}

/**
 * Fixed wording for each failure code. The same sentences as the macOS card
 * (RateLimitCardView) so a screenshot from either platform reads the same; the
 * Claude Desktop record and Credential Manager lines exist only on Windows.
 * An unknown code (a newer backend) falls back to a neutral sentence.
 */
export function quotaFailureText(failure: RateLimitLiveFailure, providerName: string): string {
  switch (failure.code) {
    case "claudeBinaryNotFound":
      return "未找到 Claude Code 程序";
    case "claudeLaunchFailed":
      return "Claude 启动失败";
    case "claudeTimedOut":
      return "Claude 配额读取超时";
    case "claudeNoResponse":
      return "Claude 没有响应配额请求";
    case "claudeNoWindows":
      return "Claude 没有返回额度窗口";
    case "desktopRecordStale":
      return "Claude 桌面端记录已过期";
    case "desktopMultipleInstalls":
      return "装了多份 Claude 桌面端，账号不明确";
    case "desktopMixedOrganizations":
      return "Claude 桌面端记录涉及多个组织，账号不明确";
    case "desktopRecordUnreadable":
      return "Claude 桌面端记录无法解析";
    case "credentialsNotFound":
      return `未找到 ${providerName} 登录信息`;
    case "credentialsRejected":
      return `${providerName} 登录已失效`;
    case "endpointTimedOut":
      return `${providerName} 配额接口请求超时`;
    case "endpointUnreachable":
      return `${providerName} 配额接口连接失败`;
    case "endpointHttpStatus":
      return failure.status != null
        ? `${providerName} 配额接口返回错误 ${failure.status}`
        : `${providerName} 配额接口返回错误`;
    case "endpointUnrecognized":
      return `${providerName} 配额接口返回了无法识别的数据`;
    case "credentialStoreUnavailable":
      return "无法读取 Windows 凭据管理器";
    case "cliRuntimeMissing":
      return "未找到 Node.js 或 Bun";
    case "cliTimedOut":
      return "配额 CLI 读取超时";
    case "cliFailed":
      return "配额 CLI 运行失败";
    case "cliUnrecognizedOutput":
      return "配额 CLI 返回了无法识别的数据";
    case "cliSchemaMismatch":
      return "配额 CLI 协议版本不兼容";
    case "cliNoResult":
      return "配额 CLI 没有返回这个产品";
    case "cliReportedFailure":
      return "实时读取失败";
    default:
      return "暂时无法读取订阅配额";
  }
}

export function quotaSourceText(source: RateLimitSource): string {
  switch (source) {
    case "live":
      return "实时读取";
    case "cache":
      return "缓存";
    case "desktopRecord":
      return "Claude 桌面端记录";
    case "localLog":
      return "本地日志";
    default:
      return String(source);
  }
}

/**
 * Status line for an enabled product whose card has no meters to draw. Only
 * ever states what the data channel actually reported: the source's own
 * verdict (`emptyReason`), the read's own fixed failure reason, local
 * detection, or — when none exists — that nothing has been read yet. Never
 * invents 「已用满」 for a source that cannot tell, and never says 「未检测到」
 * while a refresh is still in flight.
 */
export function quotaEmptyStateText(
  snapshot: ProviderRateLimit,
  isDetected: boolean,
  isRefreshing = false,
  providerName?: string,
): string {
  if (isRefreshing) return "正在读取订阅配额…";
  const name = providerName ?? snapshot.provider;
  switch (snapshot.emptyReason) {
    case "limitReached":
      return "本期订阅配额已用满 · 等待额度重置";
    case "noWindow":
      return "当前没有生效的额度窗口";
    case "notEntitled":
      // The account answered; it simply has no subscription. Name the product
      // so the line is actionable instead of looking like a broken read.
      return `未订阅 ${name}`;
    case "sessionWithoutPlanLimits":
      return "当前登录方式不含订阅额度（API Key / Bedrock / Vertex）";
  }
  if (snapshot.liveFailure) return quotaFailureText(snapshot.liveFailure, name);
  // A Desktop record from an older backend that did not name its reason.
  if (snapshot.sourceLabel) return "桌面额度记录缺失、过期或账号不明确";
  return isDetected ? "暂未读取到订阅配额数据" : "未检测到本机安装或登录";
}

/** First line of a retryable card: the fixed reason when the read named one. */
export function quotaRetryableText(snapshot: ProviderRateLimit, providerName: string): string {
  return snapshot.liveFailure
    ? quotaFailureText(snapshot.liveFailure, providerName)
    : "暂时无法读取订阅配额";
}

/**
 * The small line under a card. Mirrors the macOS rule: a successful live read
 * stays quiet; numbers shown after live failed say where they came from and
 * why; a retryable card names the channel that failed; Claude Desktop keeps
 * its own fixed label. Re-login cards and empty states add nothing here.
 */
export function quotaProvenanceText(
  snapshot: ProviderRateLimit,
  providerName: string,
): string | null {
  if (snapshot.sourceLabel) return snapshot.sourceLabel;
  if (!snapshot.source) return null;
  const source = `来源：${quotaSourceText(snapshot.source)}`;
  switch (snapshot.status.kind) {
    case "ok":
      return snapshot.liveFailure
        ? `${source} · ${quotaFailureText(snapshot.liveFailure, providerName)}`
        : null;
    case "retryableError":
      return source;
    default:
      return null;
  }
}

/**
 * Paid Codex plans have a 5-hour window, so a card without one keeps the slot
 * and says why (`codexFiveHourPlaceholder`) instead of silently showing only
 * 7d. Same plans as macOS `RateLimitCardView.expectsFiveHourWindow`, including
 * Business ProLite (vibe-usage-app#56).
 */
export function expectsCodexFiveHour(snapshot: ProviderRateLimit): boolean {
  if (snapshot.provider !== "codex") return false;
  const plan = snapshot.planLabel?.toLowerCase();
  return (
    plan === "plus" ||
    plan === "pro" ||
    plan === "prolite" ||
    plan === "business" ||
    plan === "business prolite"
  );
}

/**
 * The reserved Codex 5h slot when no 5h window came back, or null when the
 * card keeps no slot. Same rule as macOS (`RateLimitCardView.allRows`,
 * maintainer decision 2026-10-24): the live endpoint reports enforced windows
 * exhaustively, so its missing 5h window (`fiveHourNotEnforced`) means the
 * limit is off — 「不限额」; OpenAI keeps 5h off for Pro / Prolite, so those
 * plans read 「不限额」 from any source. Otherwise the numbers came from the
 * session log, where a missing window may only mean no activity for five
 * hours — 「近 5 小时无活动」. Business ProLite is not a Pro plan here.
 */
export function codexFiveHourPlaceholder(snapshot: ProviderRateLimit): string | null {
  if (snapshot.fiveHour || !expectsCodexFiveHour(snapshot)) return null;
  const plan = snapshot.planLabel?.toLowerCase();
  const unlimited = snapshot.fiveHourNotEnforced === true || plan === "pro" || plan === "prolite";
  return unlimited ? "不限额" : "近 5 小时无活动";
}

/**
 * Tab strip order: products whose monitoring is on first, then the rest — each
 * group in the user's persisted order. Mirrors `AppState.quotaTabOrder` on
 * macOS: an unknown id in the stored order is dropped, a catalog product the
 * stored order does not mention is appended, and turning a product off moves
 * its tab to the grey group instead of removing it.
 */
export function quotaTabOrder(
  catalog: RateLimitProvider[],
  enabled: RateLimitProvider[],
  storedOrder: RateLimitProvider[],
): RateLimitProvider[] {
  const known = new Set(catalog);
  const ordered = [
    ...storedOrder.filter((provider) => known.has(provider)),
    ...catalog.filter((provider) => !storedOrder.includes(provider)),
  ];
  return [
    ...ordered.filter((provider) => enabled.includes(provider)),
    ...ordered.filter((provider) => !enabled.includes(provider)),
  ];
}

/**
 * Drag-and-drop reorder, group aware exactly like macOS: dropping on a tab
 * inserts before it, dropping past the last tab lands at the end of the
 * dragged product's *own* group, and a cross-group drop normalizes back into
 * that group — "enabled first" outranks the drop. The returned array is the
 * full stored order and is already in render order.
 */
export function moveQuotaProduct(
  tabOrder: RateLimitProvider[],
  enabled: RateLimitProvider[],
  provider: RateLimitProvider,
  target: RateLimitProvider | null,
): RateLimitProvider[] {
  if (provider === target) return tabOrder;
  const order = [...tabOrder];
  const from = order.indexOf(provider);
  if (from < 0) return tabOrder;
  order.splice(from, 1);

  let destination: number;
  if (target != null) {
    const to = order.indexOf(target);
    if (to < 0) return tabOrder;
    destination = to;
  } else {
    // "Past the last tab" means the end of the dragged product's own group:
    // the slot right after the group's last member. Looking for the *first*
    // member of the other group instead would insert a grey product at the
    // start of the grey group, which is the opposite of dropping it last.
    const isEnabled = enabled.includes(provider);
    const lastSameGroup = order.reduce(
      (found, item, index) => (enabled.includes(item) === isEnabled ? index : found),
      -1,
    );
    destination = lastSameGroup < 0 ? order.length : lastSameGroup + 1;
  }
  order.splice(destination, 0, provider);
  // Normalize into render order so the stored array always reproduces the strip.
  return [
    ...order.filter((item) => enabled.includes(item)),
    ...order.filter((item) => !enabled.includes(item)),
  ];
}

/**
 * "Enabled, but not producing quota": the amber dot on a tab. A refresh in
 * flight is not a problem (the card shows its spinner), a disabled product has
 * a grey tab instead, and a product with no snapshot yet says nothing.
 * Mirrors `AppState.quotaTabShowsWarning` on macOS.
 */
export function quotaTabShowsWarning(
  provider: RateLimitProvider,
  enabled: RateLimitProvider[],
  snapshot: ProviderRateLimit | undefined,
  isRefreshing: boolean,
): boolean {
  if (isRefreshing) return false;
  if (!enabled.includes(provider)) return false;
  if (!snapshot) return false;
  return snapshot.status.kind !== "ok";
}
