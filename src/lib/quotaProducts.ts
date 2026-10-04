import {
  ProviderRateLimit,
  QuotaProduct,
  RateLimitMeter,
  RateLimitProvider,
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
 * Status line for an enabled product whose card has no meters to draw. Only
 * ever states what the data channel actually reported: the live Codex
 * endpoint's own verdict (`emptyReason`), local detection, or — when neither
 * exists — that nothing has been read yet. Never invents 「已用满」 for a source
 * that cannot tell, and never says 「未检测到」 while a refresh is still in flight.
 */
export function quotaEmptyStateText(
  snapshot: ProviderRateLimit,
  isDetected: boolean,
  isRefreshing = false,
  providerName?: string,
): string {
  if (isRefreshing) return "正在读取订阅配额…";
  if (snapshot.sourceLabel && snapshot.status.kind === "noData") return "桌面额度记录缺失、过期或账号不明确";
  switch (snapshot.emptyReason) {
    case "limitReached":
      return "本期订阅配额已用满 · 等待额度重置";
    case "noWindow":
      return "当前没有生效的额度窗口";
    case "notEntitled":
      // The account answered; it simply has no subscription. Name the product
      // so the line is actionable instead of looking like a broken read.
      return `未订阅 ${providerName ?? snapshot.provider}`;
    case "sessionWithoutPlanLimits":
      return "当前登录方式不含订阅额度（API Key / Bedrock / Vertex）";
    default:
      return isDetected ? "暂未读取到订阅配额数据" : "未检测到本机安装或登录";
  }
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
