import {
  ProviderRateLimit,
  QuotaProduct,
  RateLimitLiveFailure,
  RateLimitMeter,
  RateLimitMeterPart,
  RateLimitProvider,
  RateLimitSignIn,
  RateLimitSource,
  RateLimitWindow,
  ZCodeCredentialStatus,
  ZCodeQuotaRegion,
} from "./types";
import { elapsedPercent } from "./aggregate";
import { formatPercent, formatTimeUntil } from "./formatters";

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

/** Tooltip heading for a quota row. Same words as macOS
 *  `RateLimitCardView.tooltipTitle`: the canonical monthly label carries no
 *  day count on purpose (a calendar/subscription month is 28–31 days), so the
 *  title spells it out instead of echoing the raw token. */
export function quotaTooltipTitle(label: string): string {
  switch (label) {
    case "5h":
      return "5 小时窗口";
    case "7d":
      return "7 天窗口";
    case "Month":
      return "月度窗口";
    default:
      return label;
  }
}

/** One labelled line of a quota tooltip: `usage` is the row's own share,
 *  `time` its window or reset, `part` one piece of a meter's breakdown. */
export interface QuotaTooltipLine {
  kind: "usage" | "time" | "part";
  label: string;
  value: string;
  /** Present for `usage` and `part`: drives the colour of the usage line. */
  utilization?: number;
  /** The time is unknown: drawn dimmer. */
  muted?: boolean;
}

/** The lines under a quota tooltip's title, in macOS `TooltipView` order:
 *  Token 用量, then 时间 (window elapsed and remaining) or 重置 (only the reset
 *  countdown is known) or 时间 未知, then each breakdown part. */
export function quotaTooltipLines(
  window: RateLimitWindow,
  breakdown: RateLimitMeterPart[] = [],
  now: Date = new Date(),
): QuotaTooltipLine[] {
  const elapsed = elapsedPercent(window, now);
  const remaining =
    window.resetsAt != null ? formatTimeUntil(new Date(window.resetsAt * 1000), now) : null;
  const time: QuotaTooltipLine =
    elapsed != null && remaining != null
      ? { kind: "time", label: "时间", value: `已过去 ${formatPercent(elapsed)} · 剩余 ${remaining}` }
      : remaining != null
        ? { kind: "time", label: "重置", value: `剩余 ${remaining}` }
        : { kind: "time", label: "时间", value: "未知", muted: true };
  return [
    {
      kind: "usage",
      label: "Token 用量",
      value: `已使用 ${formatPercent(window.utilization)}`,
      utilization: window.utilization,
    },
    time,
    ...breakdown.map((part): QuotaTooltipLine => ({
      kind: "part",
      label: part.label,
      value: formatPercent(part.utilization),
      utilization: part.utilization,
    })),
  ];
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

/** Only selected products with a settled warning replace the row's usual status. */
export function quotaProductStatusText(
  product: QuotaProduct,
  enabled: RateLimitProvider[],
  snapshot?: ProviderRateLimit,
  refreshing = false,
  zCodeConfigured = false,
): string {
  if (product.availability === "pendingProtocol") {
    return product.isDetected ? "已检测 · 待接入" : "待接入";
  }
  const provider = product.provider;
  if (snapshot && quotaTabShowsWarning(provider, enabled, snapshot, refreshing)) {
    const reason = snapshot.liveFailure && failureCopy(snapshot.liveFailure, provider)[1];
    switch (snapshot.status.kind) {
      case "unauthorized":
        if (reason) return reason;
        if (provider === "zcode") return zCodeConfigured ? "Key 无效" : "需 Key";
        return provider === "opencode-go" ? "需登录" : "已失效";
      case "noData":
        if (snapshot.signIn) return snapshot.signIn === "required" ? "需登录" : "已失效";
        if (snapshot.emptyReason) {
          return {
            notEntitled: "未订阅",
            sessionWithoutPlanLimits: provider === "claudeCode" ? "未用订阅登录" : "不适用",
            nonessentialTrafficDisabled: "已禁止",
            limitReached: "已用满",
            noWindow: "无窗口",
            periodEnded: "待更新",
            noLocalRecord: "无记录",
          }[snapshot.emptyReason];
        }
        return reason ?? (snapshot.fetchedAt == null ? "未读取" : "无数据");
      case "retryableError": return reason ?? "失败";
      case "disabled": return "未启用";
      case "error": return "出错";
      case "ok": return "已暂停";
    }
  }
  if (provider === "zcode") return zCodeConfigured ? "已配置" : "待配置";
  return product.isDetected ? "已检测" : "未检测到";
}

/** Keep the card sentence and its settings short form together. */
function failureCopy(
  failure: RateLimitLiveFailure,
  provider: RateLimitProvider,
): [card: string, settings: string] {
  switch (failure.code) {
    case "claudeBinaryNotFound": return ["未找到 Claude Code", "未安装"];
    case "claudeLaunchFailed": return ["启动失败", "启动失败"];
    case "endpointTimedOut":
    case "cliTimedOut":
    case "claudeTimedOut": return ["读取超时", "超时"];
    case "claudeNoResponse": return ["没有响应", "无响应"];
    case "programNoAnswer":
    case "claudeNoWindows": return ["没有返回额度", "无响应"];
    case "desktopRecordStale": return ["Claude 桌面端记录已过期", "已过期"];
    case "desktopMultipleInstalls": return ["装了多份 Claude 桌面端，账号不明确", "不明确"];
    case "desktopMixedOrganizations": return ["Claude 桌面端记录涉及多个组织，账号不明确", "不明确"];
    case "desktopRecordUnreadable": return ["Claude 桌面端记录无法解析", "数据异常"];
    case "credentialsNotFound":
      if (provider === "opencode-go") return ["在 OpenCode 里登录后自动显示", "需登录"];
      if (provider === "grok") return ["运行 grok login 后可实时读取", "需登录"];
      return ["未找到登录信息", provider === "zcode" ? "需 Key" : "需登录"];
    case "credentialsExpired":
      if (provider === "opencode-go") return ["打开 OpenCode 后自动显示", "需打开"];
      return ["登录已失效", "已失效"];
    // AGY reports "sign in" the same way whether it was never signed in or lapsed.
    case "credentialsRejected":
      return ["登录已失效", provider === "zcode" ? "Key 无效" : provider === "antigravity" ? "需登录" : "已失效"];
    case "credentialsUnreadable": return ["读不到登录信息", "读不了"];
    case "credentialsEncrypted": return ["登录信息已加密，暂时读不了", "不支持"];
    case "loginRefreshFailed": return ["登录续期失败", "续期失败"];
    case "programNotFound":
      if (provider === "antigravity") return ["请安装 agy 或 Antigravity", "未安装"];
      if (provider === "grok") return ["未找到 Grok 命令行", "未安装"];
      return ["未找到程序", "未安装"];
    case "programOutdated": return ["版本较旧，升级后可实时读取", "需升级"];
    case "localRecordUnreadable": return ["读不了本机记录", "读不了"];
    case "endpointUnreachable": return ["连接失败", "连不上"];
    case "endpointHttpStatus":
      return [failure.status == null ? "接口报错" : `接口报错 ${failure.status}`, "接口报错"];
    case "endpointUnrecognized": return ["返回数据无法识别", "数据异常"];
    case "credentialStoreUnavailable": return ["无法读取 Windows 凭据管理器", "读不了"];
    case "cliRuntimeMissing": return ["缺少 Node.js", "缺 Node"];
    case "cliFailed": return ["配额 CLI 出错", "CLI 出错"];
    case "cliUnrecognizedOutput": return ["配额 CLI 输出无法识别", "数据异常"];
    case "cliSchemaMismatch": return ["配额 CLI 版本不兼容", "CLI 出错"];
    case "cliNoResult": return ["配额 CLI 没有返回结果", "CLI 出错"];
    case "cliReportedFailure": return ["实时读取失败", "失败"];
    case "egressRegionUnsupported": return [`网络出口${egressPlace(failure.region)}，已暂停读取`, "已暂停"];
    case "egressUnconfirmed": return ["无法确认网络出口，已暂停读取", "已暂停"];
    case "proxyNotDetected": return ["未检测到代理，已暂停读取", "已暂停"];
    case "proxyUnsupported": return ["代理无法照着走，已暂停读取", "已暂停"];
    default: return ["暂时读不到额度", "失败"];
  }
}

export function quotaFailureText(failure: RateLimitLiveFailure, provider: RateLimitProvider): string {
  return failureCopy(failure, provider)[0];
}

export function quotaAuthorizationText(snapshot: ProviderRateLimit, region: ZCodeQuotaRegion): string {
  const { provider, liveFailure } = snapshot;
  const missing = liveFailure?.code === "credentialsNotFound";
  if (liveFailure && !missing && liveFailure.code !== "credentialsRejected") {
    return quotaFailureText(liveFailure, provider);
  }
  switch (provider) {
    case "zcode":
      return missing
        ? `请在设置中配置 ${region === "bigModel" ? "BigModel" : "Z.ai"} API Key`
        : "API Key 无效，请在设置中更换";
    case "opencode-go": return "请在 OpenCode 中重新登录后重试";
    case "antigravity": return "请在 Antigravity 中登录";
    case "kimi-code": if (missing) return "请先登录 Kimi Code"; break;
    case "cursor": if (missing) return "请先登录 Cursor"; break;
  }
  return missing ? "请先登录" : "登录已失效，请重新登录";
}

/** Where an unsupported egress is, in the card's words (same as macOS). */
export function egressPlace(region?: string): string {
  switch (region) {
    case "CN":
      return "在中国大陆";
    case "HK":
      return "在香港";
    case "MO":
      return "在澳门";
    default:
      return "不在支持地区";
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
  if (snapshot.signIn) return quotaSignInText(snapshot.signIn);
  const name = providerName ?? snapshot.provider;
  switch (snapshot.emptyReason) {
    case "limitReached":
      return "额度已用满 · 等待重置";
    case "noWindow":
      return "没有生效的额度窗口";
    case "notEntitled":
      // The account answered; it simply has no subscription. Name the product
      // so the line is actionable instead of looking like a broken read.
      return `未订阅 ${name}`;
    case "sessionWithoutPlanLimits":
      // Claude Code answers `rate_limits_available: false` for a token (a
      // gateway or relay), an API key, a cloud provider, and for no sign-in at
      // all; what they share is that it is not using a subscription sign-in.
      // The subscription itself may be fine.
      return snapshot.provider === "claudeCode" ? "Claude Code 未使用订阅登录" : "当前登录方式不含订阅额度";
    case "periodEnded":
      if (snapshot.provider === "grok") return "账期已结束 · 用一次 Grok 后更新";
      if (snapshot.provider === "antigravity") return "额度已重置 · 下次读取时更新";
      return "上个周期已结束";
    case "noLocalRecord":
      if (snapshot.provider === "grok") return "暂无记录 · 用一次 Grok 后更新";
      if (snapshot.provider === "antigravity") return "用过 Antigravity 后显示";
      return "本机暂无记录";
    case "nonessentialTrafficDisabled":
      return "组织策略关闭了 Claude Code 非必要流量 · 无法读取";
  }
  if (snapshot.liveFailure) return quotaFailureText(snapshot.liveFailure, snapshot.provider);
  // A Desktop record from an older backend that did not name its reason.
  if (snapshot.sourceLabel) return "桌面额度记录缺失、过期或账号不明确";
  if (snapshot.fetchedAt == null) return "尚未读取";
  return isDetected ? "没读到额度，原因未知" : "未检测到安装";
}

/** The Cursor card's sign-in prompt: only what the CLI asked for (`signIn`).
 *  Same words as macOS; a click opens the Cursor row in Settings. */
export function quotaSignInText(signIn: RateLimitSignIn): string {
  return signIn === "required" ? "需要登录 Cursor" : "Cursor 登录已失效";
}

/** First line of a retryable card: the fixed reason when the read named one. */
export function quotaRetryableText(snapshot: ProviderRateLimit): string {
  return snapshot.liveFailure
    ? quotaFailureText(snapshot.liveFailure, snapshot.provider)
    : "暂时读不到额度";
}

/**
 * The small line under a card. Mirrors the macOS rule: a successful live read
 * stays quiet; numbers shown after live failed say where they came from and
 * why; a retryable card names the channel that failed; Claude Desktop keeps
 * its own fixed label. Re-login cards and empty states add nothing here.
 */
export function quotaProvenanceText(snapshot: ProviderRateLimit): string | null {
  if (snapshot.sourceLabel) return snapshot.sourceLabel;
  if (snapshot.status.kind === "retryableError") {
    return `来源：${quotaSourceText(snapshot.source ?? "live")}`;
  }
  if (snapshot.status.kind !== "ok" || !snapshot.liveFailure) return null;
  const reason = quotaFailureText(snapshot.liveFailure, snapshot.provider);
  if (!snapshot.source) return reason;
  const source = snapshot.source === "live" ? "上次实时读取" : quotaSourceText(snapshot.source);
  return `来源：${source} · ${reason}`;
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
  if (snapshot?.provider !== provider) return false;
  // Numbers kept on the card while the egress check pauses the read still
  // warn: they are not being refreshed (same as macOS).
  return snapshot.status.kind !== "ok" || isQuotaPause(snapshot.liveFailure);
}

/** A read paused by an egress check or the CLI's proxy guard. */
export function isQuotaPause(failure?: RateLimitLiveFailure | null): boolean {
  return (
    failure?.code === "egressRegionUnsupported" ||
    failure?.code === "egressUnconfirmed" ||
    failure?.code === "proxyNotDetected" ||
    failure?.code === "proxyUnsupported"
  );
}
