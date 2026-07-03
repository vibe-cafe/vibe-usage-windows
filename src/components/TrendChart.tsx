// Hourly/daily trend chart (stacked bars) — port of Views/BarChartView.swift.
// Self-drawn (divs) rather than a chart lib so segment colors, 2px gaps,
// top-radius and tooltip match the SwiftUI original exactly.

import { useMemo, useRef, useState } from "react";
import { useAppState } from "../state/AppStateContext";
import {
  BarData,
  barTotal,
  buildChartData,
  filterBuckets,
  filterSessions,
  labelInterval,
} from "../lib/aggregate";
import {
  formatCost,
  formatDateShort,
  formatDuration,
  formatHourShort,
  formatNumber,
} from "../lib/formatters";
import { CHART_MODE_LABELS, ChartMode, isHourly } from "../lib/types";

const CHART_HEIGHT = 150;
const Y_AXIS_WIDTH = 44;
const X_LABEL_WIDTH = 46;

export function TrendChart() {
  const state = useAppState();
  const hourly = isHourly(state.timeRange);

  const data = useMemo(() => {
    const buckets = filterBuckets(state.buckets, state.filters, state.timeRange);
    const sessions = filterSessions(state.sessions, state.filters, state.timeRange);
    return buildChartData(
      buckets,
      sessions,
      state.timeRange,
      state.visibleDayCount,
      state.timeRange === "custom" ? state.normalizedCustomRange.to : null,
    );
  }, [
    state.buckets,
    state.sessions,
    state.filters,
    state.timeRange,
    state.visibleDayCount,
    state.normalizedCustomRange,
  ]);

  const maxTotal = Math.max(...data.map(barTotal), 1);
  const maxCost = Math.max(...data.map((b) => b.cost), 0.001);
  const maxActiveMinutes = Math.max(...data.map((b) => b.activeMinutes), 0.1);
  const interval = Math.max(labelInterval(data.length, hourly), 1);
  const visibleLabelIndices = useMemo(() => {
    const out: number[] = [];
    for (let i = 0; i < data.length; i += interval) out.push(i);
    return out;
  }, [data.length, interval]);

  return (
    <div className="w-full rounded-card border border-card-border bg-card p-[14px]">
      {/* Header */}
      <div className="flex items-center pb-[14px]">
        <span className="text-[13px] font-medium text-t-secondary">
          {hourly ? "每小时趋势" : "每日趋势"}
        </span>
        <div className="grow" />
        <div className="flex shrink-0 rounded-full p-0.5" style={{ background: "#292929", gap: 2 }}>
          {(Object.keys(CHART_MODE_LABELS) as ChartMode[]).map((mode) => {
            const active = state.chartMode === mode;
            return (
              <button
                key={mode}
                onClick={() => state.setChartMode(mode)}
                className="rounded-full px-2.5 py-1 text-[11px]"
                style={{
                  background: active ? "#474747" : "transparent",
                  color: active ? "#FFFFFF" : "#808080",
                  fontWeight: active ? 500 : 400,
                }}
              >
                {CHART_MODE_LABELS[mode]}
              </button>
            );
          })}
        </div>
      </div>

      <ChartContent
        data={data}
        chartMode={state.chartMode}
        hourly={hourly}
        maxTotal={maxTotal}
        maxCost={maxCost}
        maxActiveMinutes={maxActiveMinutes}
        visibleLabelIndices={visibleLabelIndices}
      />
    </div>
  );
}

function ChartContent({
  data,
  chartMode,
  hourly,
  maxTotal,
  maxCost,
  maxActiveMinutes,
  visibleLabelIndices,
}: {
  data: BarData[];
  chartMode: ChartMode;
  hourly: boolean;
  maxTotal: number;
  maxCost: number;
  maxActiveMinutes: number;
  visibleLabelIndices: number[];
}) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const plotRef = useRef<HTMLDivElement>(null);

  const yAxisTop =
    chartMode === "token"
      ? formatNumber(maxTotal)
      : chartMode === "cost"
        ? formatCost(maxCost)
        : formatDuration(Math.floor(maxActiveMinutes * 60));

  const onMove = (e: React.MouseEvent) => {
    const el = plotRef.current;
    if (!el || data.length === 0) return;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0) return;
    const barW = rect.width / data.length;
    const idx = Math.min(Math.max(Math.floor((e.clientX - rect.left) / barW), 0), data.length - 1);
    setHoveredIndex(idx);
  };

  const plotWidth = plotRef.current?.getBoundingClientRect().width ?? 0;

  return (
    <div>
      {/* Chart row: Y axis + bars */}
      <div className="flex items-end" style={{ gap: 6 }}>
        <div
          className="flex shrink-0 flex-col justify-between text-right font-mono text-[11px] text-t-tertiary"
          style={{ width: Y_AXIS_WIDTH, height: CHART_HEIGHT }}
        >
          <span className="truncate">{yAxisTop}</span>
          <span>0</span>
        </div>

        <div
          ref={plotRef}
          className="relative flex min-w-0 grow items-end"
          style={{ height: CHART_HEIGHT, gap: 2 }}
          onMouseMove={onMove}
          onMouseLeave={() => setHoveredIndex(null)}
        >
          {data.map((bar) => (
            <Bar
              key={bar.id}
              bar={bar}
              chartMode={chartMode}
              maxTotal={maxTotal}
              maxCost={maxCost}
              maxActiveMinutes={maxActiveMinutes}
            />
          ))}

          {hoveredIndex != null && data[hoveredIndex] && plotWidth > 0 && (
            <ChartTooltip
              bar={data[hoveredIndex]}
              chartMode={chartMode}
              hourly={hourly}
              x={clamp(
                (plotWidth / data.length) * (hoveredIndex + 0.5),
                Math.min(80, plotWidth / 2),
                Math.max(Math.min(80, plotWidth / 2), plotWidth - Math.min(80, plotWidth / 2)),
              )}
            />
          )}
        </div>
      </div>

      {/* X axis */}
      <div className="flex pt-2" style={{ height: 24 }}>
        <div className="shrink-0" style={{ width: Y_AXIS_WIDTH + 6 }} />
        <div className="relative min-w-0 grow overflow-hidden" style={{ height: 16 }}>
          {plotWidth > 0 &&
            visibleLabelIndices.map((index) => {
              const bar = data[index];
              if (!bar) return null;
              const raw = (plotWidth * (index + 0.5)) / data.length;
              const inset = Math.min(X_LABEL_WIDTH / 2, plotWidth / 2);
              const x = clamp(raw, inset, Math.max(inset, plotWidth - inset));
              return (
                <span
                  key={bar.id}
                  className="absolute top-0 text-center text-[11px] text-t-muted"
                  style={{ width: X_LABEL_WIDTH, left: x - X_LABEL_WIDTH / 2 }}
                >
                  {hourly ? formatHourShort(bar.id) : formatDateShort(bar.id)}
                </span>
              );
            })}
        </div>
      </div>
    </div>
  );
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), hi);
}

function Bar({
  bar,
  chartMode,
  maxTotal,
  maxCost,
  maxActiveMinutes,
}: {
  bar: BarData;
  chartMode: ChartMode;
  maxTotal: number;
  maxCost: number;
  maxActiveMinutes: number;
}) {
  const topRadius = { borderTopLeftRadius: 2, borderTopRightRadius: 2 };

  if (chartMode === "token") {
    const outputH = (bar.output / maxTotal) * CHART_HEIGHT;
    const inputH = (bar.input / maxTotal) * CHART_HEIGHT;
    const cachedH = (bar.cached / maxTotal) * CHART_HEIGHT;
    return (
      <div className="flex min-w-0 flex-1 flex-col justify-end" style={{ height: CHART_HEIGHT }}>
        <div style={{ height: outputH, background: "rgba(255,255,255,0.9)", ...topRadius }} />
        <div style={{ height: inputH, background: "rgba(255,255,255,0.5)" }} />
        <div style={{ height: cachedH, background: "rgba(255,255,255,0.24)" }} />
      </div>
    );
  }

  const h =
    chartMode === "cost"
      ? (bar.cost / maxCost) * CHART_HEIGHT
      : (bar.activeMinutes / maxActiveMinutes) * CHART_HEIGHT;
  const color = chartMode === "cost" ? "#33CC80" : "#6199FF";
  return (
    <div className="flex min-w-0 flex-1 flex-col justify-end" style={{ height: CHART_HEIGHT }}>
      <div style={{ height: h, background: color, ...topRadius }} />
    </div>
  );
}

function ChartTooltip({
  bar,
  chartMode,
  hourly,
  x,
}: {
  bar: BarData;
  chartMode: ChartMode;
  hourly: boolean;
  x: number;
}) {
  return (
    <div
      className="pointer-events-none absolute z-10 flex flex-col gap-[3px] whitespace-nowrap rounded-card bg-black p-2 text-[11px] shadow-[0_2px_4px_rgba(0,0,0,0.5)]"
      style={{
        left: x,
        top: 40,
        transform: "translate(-50%, -50%)",
        border: "0.5px solid #333333",
      }}
    >
      <span className="font-medium text-white">
        {hourly ? formatHourShort(bar.id) : formatDateShort(bar.id)}
      </span>
      {chartMode === "token" && (
        <>
          <span style={{ color: "#CCCCCC" }}>总 Token: {formatNumber(barTotal(bar))}</span>
          <span className="flex gap-2" style={{ color: "#808080" }}>
            <span>输入: {formatNumber(bar.input)}</span>
            <span>输出: {formatNumber(bar.output)}</span>
          </span>
          {bar.cached > 0 && (
            <span style={{ color: "#737373" }}>缓存: {formatNumber(bar.cached)}</span>
          )}
          <span style={{ color: "#33CC80" }}>费用: {formatCost(bar.cost)}</span>
        </>
      )}
      {chartMode === "cost" && <span style={{ color: "#33CC80" }}>费用: {formatCost(bar.cost)}</span>}
      {chartMode === "activeTime" && (
        <span style={{ color: "#6199FF" }}>
          活跃时长: {formatDuration(Math.floor(bar.activeMinutes * 60))}
        </span>
      )}
    </div>
  );
}
