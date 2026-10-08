// 4 stat cards — port of Views/SummaryCardsView.swift.

import { useMemo, useState } from "react";
import { useAppState } from "../state/AppStateContext";
import { filterBuckets, filterSessions, summarize } from "../lib/aggregate";
import {
  formatChineseTokens,
  formatCnyCost,
  formatCost,
  formatDuration,
  formatNumber,
} from "../lib/formatters";

type CurrencyMode = "usd" | "cny";
type TokenMode = "international" | "chinese";

function formatExactInteger(value: number): string {
  return Number.isFinite(value) ? Math.trunc(value).toLocaleString("zh-CN") : "—";
}

export function SummaryCards() {
  const state = useAppState();
  const [currencyMode, setCurrencyMode] = useState<CurrencyMode>("usd");
  const [totalTokenMode, setTotalTokenMode] = useState<TokenMode>("international");
  const [cachedTokenMode, setCachedTokenMode] = useState<TokenMode>("international");

  const totals = useMemo(() => {
    const buckets = filterBuckets(state.buckets, state.filters, state.timeRange, state.names);
    const sessions = filterSessions(state.sessions, state.filters, state.timeRange);
    return summarize(buckets, sessions);
  }, [state.buckets, state.sessions, state.filters, state.timeRange, state.names]);

  return (
    <div className="flex w-full items-start gap-2">
      <StatCard
        label="预估费用"
        value={currencyMode === "usd" ? formatCost(totals.totalCost) : formatCnyCost(totals.totalCost)}
        color="#33CC80"
        onClick={() => setCurrencyMode((mode) => (mode === "usd" ? "cny" : "usd"))}
        pressed={currencyMode === "cny"}
        title={`点击切换美元/人民币；精确值：${formatCost(totals.totalCost)}`}
        ariaLabel={`预估费用，当前显示${currencyMode === "usd" ? "美元" : "人民币"}，点击切换；精确美元值 ${formatCost(totals.totalCost)}`}
      />
      <StatCard
        label="总 Token"
        value={totalTokenMode === "international" ? formatNumber(totals.totalTokens) : formatChineseTokens(totals.totalTokens)}
        onClick={() => setTotalTokenMode((mode) => (mode === "international" ? "chinese" : "international"))}
        pressed={totalTokenMode === "chinese"}
        title={`点击切换国际/中文单位；精确值：${formatExactInteger(totals.totalTokens)}`}
        ariaLabel={`总 Token，当前显示${totalTokenMode === "international" ? "国际单位" : "中文单位"}，点击切换；精确值 ${formatExactInteger(totals.totalTokens)}`}
      />
      <StatCard
        label="缓存 Token"
        value={cachedTokenMode === "international" ? formatNumber(totals.totalCachedInputTokens) : formatChineseTokens(totals.totalCachedInputTokens)}
        onClick={() => setCachedTokenMode((mode) => (mode === "international" ? "chinese" : "international"))}
        pressed={cachedTokenMode === "chinese"}
        title={`点击切换国际/中文单位；精确值：${formatExactInteger(totals.totalCachedInputTokens)}`}
        ariaLabel={`缓存 Token，当前显示${cachedTokenMode === "international" ? "国际单位" : "中文单位"}，点击切换；精确值 ${formatExactInteger(totals.totalCachedInputTokens)}`}
      />
      <StatCard
        label="活跃时长"
        value={formatDuration(totals.totalActiveSeconds)}
        color="#6199FF"
      />
    </div>
  );
}

function StatCard({
  label,
  value,
  color = "#FFFFFF",
  onClick,
  pressed,
  title,
  ariaLabel,
}: {
  label: string;
  value: string;
  color?: string;
  onClick?: () => void;
  pressed?: boolean;
  title?: string;
  ariaLabel?: string;
}) {
  const className = [
    "min-w-0 flex-1 rounded-card border border-card-border bg-card px-[11px] py-[13px] text-left",
    onClick &&
      "cursor-pointer transition-colors hover:border-[#3A3A3A] active:bg-[#1D1D1D] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-white/20",
  ]
    .filter(Boolean)
    .join(" ");

  const content = (
    <>
      <div className="h-3.5 truncate text-xs leading-[14px] text-t-secondary">{label}</div>
      <div
        className="value-transition mt-1.5 h-6 overflow-hidden whitespace-nowrap font-sans tabular-nums text-xl font-bold leading-6"
        style={{ color }}
      >
        {value}
      </div>
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        className={className}
        onClick={onClick}
        aria-label={ariaLabel}
        aria-pressed={pressed}
        title={title}
      >
        {content}
      </button>
    );
  }

  return <div className={className}>{content}</div>;
}
