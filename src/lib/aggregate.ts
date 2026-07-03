// Client-side filtering & aggregation — 1:1 port of the filter/aggregate logic
// embedded in SummaryCardsView / BarChartView / DistributionChartsView /
// AppState.filteredSessions.

import {
  UsageBucket,
  UsageSession,
  FilterState,
  TimeRange,
  bucketDate,
  bucketDayKey,
  bucketHourKey,
  computedTotal,
  isHourly,
  sessionDate,
  sessionDayKey,
  sessionHourKey,
  startCutoff,
} from "./types";
import { localDayKey } from "./formatters";

/** Shared bucket predicate (cutoff + 4-dimension filters). */
export function filterBuckets(
  buckets: UsageBucket[],
  filters: FilterState,
  range: TimeRange,
  now: Date = new Date(),
): UsageBucket[] {
  const cutoff = startCutoff(range, now);
  return buckets.filter((bucket) => {
    if (cutoff) {
      const date = bucketDate(bucket);
      if (date && date < cutoff) return false;
    }
    if (filters.sources.size > 0 && !filters.sources.has(bucket.source)) return false;
    if (filters.models.size > 0 && !filters.models.has(bucket.model)) return false;
    if (filters.projects.size > 0 && !filters.projects.has(bucket.project)) return false;
    if (filters.hostnames.size > 0 && !filters.hostnames.has(bucket.hostname)) return false;
    return true;
  });
}

/**
 * Session filter — NOTE: the model filter intentionally does NOT apply to
 * sessions (mirrors AppState.filteredSessions; sessions carry no model).
 */
export function filterSessions(
  sessions: UsageSession[],
  filters: FilterState,
  range: TimeRange,
  now: Date = new Date(),
): UsageSession[] {
  const cutoff = startCutoff(range, now);
  return sessions.filter((session) => {
    if (cutoff) {
      const date = sessionDate(session);
      if (date && date < cutoff) return false;
    }
    if (filters.sources.size > 0 && !filters.sources.has(session.source)) return false;
    if (filters.projects.size > 0 && !filters.projects.has(session.project)) return false;
    if (filters.hostnames.size > 0 && !filters.hostnames.has(session.hostname)) return false;
    return true;
  });
}

// ---------------------------------------------------------------------------
// Summary totals (SummaryCardsView)

export interface SummaryTotals {
  totalCost: number;
  totalTokens: number;
  totalCachedInputTokens: number;
  totalActiveSeconds: number;
}

export function summarize(buckets: UsageBucket[], sessions: UsageSession[]): SummaryTotals {
  let totalCost = 0;
  let totalTokens = 0;
  let totalCachedInputTokens = 0;
  for (const b of buckets) {
    totalCost += b.estimatedCost ?? 0;
    totalTokens += computedTotal(b);
    totalCachedInputTokens += b.cachedInputTokens;
  }
  let totalActiveSeconds = 0;
  for (const s of sessions) {
    totalActiveSeconds += s.activeSeconds;
  }
  return { totalCost, totalTokens, totalCachedInputTokens, totalActiveSeconds };
}

// ---------------------------------------------------------------------------
// Trend chart data (BarChartView.chartData)

export interface BarData {
  id: string; // dayKey or hourKey
  input: number;
  output: number;
  cached: number;
  cost: number;
  activeMinutes: number;
}

export function barTotal(bar: BarData): number {
  return bar.input + bar.output + bar.cached;
}

function emptyBar(id: string): BarData {
  return { id, input: 0, output: 0, cached: 0, cost: 0, activeMinutes: 0 };
}

/** UTC hourKey (yyyy-MM-ddTHH) for a Date — matches ISO8601 prefix(13). */
function utcHourKey(date: Date): string {
  return date.toISOString().slice(0, 13);
}

export function buildChartData(
  filteredBuckets: UsageBucket[],
  filteredSessions: UsageSession[],
  range: TimeRange,
  visibleDayCount: number,
  customTo: Date | null,
  now: Date = new Date(),
): BarData[] {
  const hourly = isHourly(range);
  const map = new Map<string, BarData>();

  for (const bucket of filteredBuckets) {
    const key = hourly ? bucketHourKey(bucket) : bucketDayKey(bucket);
    let bar = map.get(key);
    if (!bar) {
      bar = emptyBar(key);
      map.set(key, bar);
    }
    bar.input += bucket.inputTokens;
    // Reasoning tokens are priced as output — three tiers: input/output/cache read.
    bar.output += bucket.outputTokens + bucket.reasoningOutputTokens;
    bar.cached += bucket.cachedInputTokens;
    bar.cost += bucket.estimatedCost ?? 0;
  }

  for (const session of filteredSessions) {
    const key = hourly ? sessionHourKey(session) : sessionDayKey(session);
    let bar = map.get(key);
    if (!bar) {
      bar = emptyBar(key);
      map.set(key, bar);
    }
    bar.activeMinutes += session.activeSeconds / 60.0;
  }

  if (hourly) {
    // `.today`: local midnight → now (slot count grows 1→24).
    // `.oneDay`: 24-slot rolling window ending at the current hour.
    const currentHour = new Date(now);
    currentHour.setMinutes(0, 0, 0);
    let start: Date;
    if (range === "today") {
      start = new Date(now);
      start.setHours(0, 0, 0, 0);
    } else {
      start = new Date(currentHour.getTime() - 23 * 3600_000);
    }

    const result: BarData[] = [];
    for (let t = start.getTime(); t <= currentHour.getTime(); t += 3600_000) {
      const key = utcHourKey(new Date(t));
      result.push(map.get(key) ?? emptyBar(key));
    }
    return result;
  }

  // Daily: fill all days in range ending at today (or custom `to`).
  const endDay = customTo ? new Date(customTo) : new Date(now);
  endDay.setHours(0, 0, 0, 0);
  const result: BarData[] = [];
  for (let i = visibleDayCount - 1; i >= 0; i--) {
    const date = new Date(endDay.getTime() - i * 86400_000);
    // Re-normalize across DST shifts
    date.setHours(0, 0, 0, 0);
    const key = localDayKey(date);
    result.push(map.get(key) ?? emptyBar(key));
  }
  return result;
}

/** X-axis label interval — mirrors BarChartView.labelInterval */
export function labelInterval(count: number, hourly: boolean): number {
  if (hourly) {
    if (count <= 12) return 3;
    if (count <= 18) return 4;
    return 6;
  }
  if (count <= 3) return 1;
  if (count <= 7) return 1;
  if (count <= 15) return 3;
  if (count <= 45) return 7;
  if (count <= 100) return 14;
  return 30;
}

// ---------------------------------------------------------------------------
// Distribution donuts (DistributionChartsView.aggregate)

export interface SliceData {
  label: string;
  tokens: number;
  cost: number;
  /** hex color */
  color: string;
}

export const SLICE_COLORS = [
  "#3B82F6", // (0.23, 0.51, 0.96)
  "#0FBA83", // (0.06, 0.73, 0.51)
  "#F59E0B", // (0.96, 0.62, 0.04)
  "#F04545", // (0.94, 0.27, 0.27)
  "#8C5CF5", // (0.55, 0.36, 0.96)
  "#ED4D99", // (0.93, 0.30, 0.60)
];

export const OTHER_SLICE_COLOR = "#525252"; // Color(white: 0.32)

export function aggregateSlices(
  buckets: UsageBucket[],
  key: (b: UsageBucket) => string,
): SliceData[] {
  const map = new Map<string, { tokens: number; cost: number }>();
  for (const b of buckets) {
    const raw = key(b);
    const k = raw === "" ? "未知" : raw;
    const existing = map.get(k) ?? { tokens: 0, cost: 0 };
    existing.tokens += computedTotal(b);
    existing.cost += b.estimatedCost ?? 0;
    map.set(k, existing);
  }

  const sorted = [...map.entries()].sort((a, b) => b[1].tokens - a[1].tokens);

  const slices: SliceData[] = [];
  let otherTokens = 0;
  let otherCost = 0;

  sorted.forEach(([label, v], i) => {
    if (i < 6) {
      slices.push({ label, tokens: v.tokens, cost: v.cost, color: SLICE_COLORS[i % SLICE_COLORS.length] });
    } else {
      otherTokens += v.tokens;
      otherCost += v.cost;
    }
  });

  if (otherTokens > 0) {
    slices.push({ label: "其他", tokens: otherTokens, cost: otherCost, color: OTHER_SLICE_COLOR });
  }

  return slices;
}

// ---------------------------------------------------------------------------
// Rate-limit window display helpers (RateLimitCardView extensions)

import { RateLimitWindow } from "./types";

/** Fraction of the rolling window that has elapsed; null if components missing. */
export function elapsedPercent(w: RateLimitWindow, now: Date = new Date()): number | null {
  if (w.resetsAt == null || w.windowDuration == null || w.windowDuration <= 0) return null;
  const remaining = Math.max(0, w.resetsAt * 1000 - now.getTime()) / 1000;
  const elapsed = Math.max(0, w.windowDuration - remaining);
  return Math.min(100, (elapsed / w.windowDuration) * 100);
}

/** Progress-bar color tier — mirrors ProgressBar.color(for:) */
export function utilizationColor(utilization: number): string {
  if (utilization < 70) return "#D9D9D9"; // Color(white: 0.85)
  if (utilization < 90) return "#F59E0B";
  return "#F04545";
}
