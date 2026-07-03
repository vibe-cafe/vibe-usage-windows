// 4 stat cards — port of Views/SummaryCardsView.swift.

import { useMemo } from "react";
import { useAppState } from "../state/AppStateContext";
import { filterBuckets, filterSessions, summarize } from "../lib/aggregate";
import { formatCost, formatDuration, formatNumber } from "../lib/formatters";

export function SummaryCards() {
  const state = useAppState();

  const totals = useMemo(() => {
    const buckets = filterBuckets(state.buckets, state.filters, state.timeRange);
    const sessions = filterSessions(state.sessions, state.filters, state.timeRange);
    return summarize(buckets, sessions);
  }, [state.buckets, state.sessions, state.filters, state.timeRange]);

  return (
    <div className="flex w-full items-start gap-2">
      <StatCard label="预估费用" value={formatCost(totals.totalCost)} color="#33CC80" />
      <StatCard label="总 Token" value={formatNumber(totals.totalTokens)} />
      <StatCard label="缓存 Token" value={formatNumber(totals.totalCachedInputTokens)} />
      <StatCard
        label="活跃时长"
        value={formatDuration(totals.totalActiveSeconds)}
        color="#6199FF"
      />
    </div>
  );
}

function StatCard({ label, value, color = "#FFFFFF" }: { label: string; value: string; color?: string }) {
  return (
    <div className="min-w-0 flex-1 rounded-card border border-card-border bg-card px-[11px] py-[13px]">
      <div className="h-3.5 truncate text-xs leading-[14px] text-t-secondary">{label}</div>
      <div
        className="value-transition mt-1.5 h-6 overflow-hidden whitespace-nowrap font-mono text-xl font-bold leading-6"
        style={{ color }}
      >
        {value}
      </div>
    </div>
  );
}
