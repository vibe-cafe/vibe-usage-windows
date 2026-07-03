// 1:1 port of VibeUsage/Utils/Formatters.swift

/** Format large numbers with compact notation: 1234 → "1,234", 45200 → "45.2K" */
export function formatNumber(n: number): string {
  if (n >= 1_000_000) {
    return `${(n / 1_000_000).toFixed(1)}M`;
  }
  if (n >= 10_000) {
    return `${(n / 1_000).toFixed(1)}K`;
  }
  return n.toLocaleString("en-US");
}

/** Format cost: $0.00, $12.34, or $0.0012 for very small values */
export function formatCost(cost: number): string {
  if (cost === 0) return "$0.00";
  if (cost < 0.01) return `$${cost.toFixed(4)}`;
  return `$${cost.toFixed(2)}`;
}

/** Format date for chart axis: "2026-02-25" → "2/25" */
export function formatDateShort(dateString: string): string {
  const parts = dateString.split("-");
  if (parts.length >= 3) {
    const month = parseInt(parts[1], 10) || 0;
    const day = parseInt(parts[2].slice(0, 2), 10) || 0;
    return `${month}/${day}`;
  }
  return dateString;
}

/** Format relative time: "刚刚", "3 分钟前", "1 小时前" */
export function formatRelativeTime(date: Date, now: Date = new Date()): string {
  const interval = (now.getTime() - date.getTime()) / 1000;
  if (interval < 60) return "刚刚";
  if (interval < 3600) return `${Math.floor(interval / 60)} 分钟前`;
  if (interval < 86400) return `${Math.floor(interval / 3600)} 小时前`;
  return `${Math.floor(interval / 86400)} 天前`;
}

/** Format hour key for chart axis: "yyyy-MM-ddTHH" (UTC) → local "15:00" */
export function formatHourShort(hourKey: string): string {
  const date = new Date(`${hourKey}:00:00Z`);
  if (isNaN(date.getTime())) return hourKey;
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/** Format duration in seconds: 90 → "1m", 3661 → "1h 1m", 86400+ → "1d 2h" */
export function formatDuration(seconds: number): string {
  if (seconds <= 0) return "0m";
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);

  if (days > 0) {
    return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  }
  if (hours > 0) {
    return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  }
  return `${Math.max(minutes, 1)}m`;
}

/** Format the gap between now and a future date: "12m", "2h 14m", "4d 18h", "已重置" */
export function formatTimeUntil(date: Date, now: Date = new Date()): string {
  const interval = Math.floor((date.getTime() - now.getTime()) / 1000);
  if (interval <= 0) return "已重置";
  return formatDuration(interval);
}

/** Parse "yyyy-MM-dd" to a local-midnight Date */
export function dateFromDayKey(key: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!m) return null;
  return new Date(parseInt(m[1], 10), parseInt(m[2], 10) - 1, parseInt(m[3], 10));
}

/** Local "yyyy-MM-dd" for a Date (used for daily chart slot keys) */
export function localDayKey(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/**
 * Donut center label — mirrors DonutShape.centerLabel which also handles the
 * billions tier (unlike formatNumber).
 */
export function formatCenterTokens(total: number): string {
  const t = Math.trunc(total);
  if (t >= 1_000_000_000) return `${(t / 1_000_000_000).toFixed(1)}B`;
  if (t >= 1_000_000) return `${(t / 1_000_000).toFixed(1)}M`;
  if (t >= 1_000) return `${(t / 1_000).toFixed(1)}K`;
  return `${t}`;
}

/** Rate-limit percent text — mirrors RateLimitWindow.percentText */
export function formatPercent(utilization: number): string {
  if (utilization < 0.05) return "0%";
  if (utilization < 1) return `${utilization.toFixed(1)}%`;
  return `${Math.round(utilization)}%`;
}

/** Donut legend percentage — mirrors DonutCardView.percentage */
export function formatSlicePercent(value: number, total: number): string {
  if (total <= 0) return "0%";
  const pct = (value / total) * 100;
  if (pct < 0.1) return "<0.1%";
  return `${pct.toFixed(1)}%`;
}
