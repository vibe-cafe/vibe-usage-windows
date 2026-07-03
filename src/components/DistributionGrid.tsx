// 2×2 donut distribution cards — port of Views/DistributionChartsView.swift.

import { useMemo, useState } from "react";
import { Cpu, Folder, Monitor, SquareTerminal } from "lucide-react";
import { useAppState } from "../state/AppStateContext";
import { aggregateSlices, filterBuckets, SliceData } from "../lib/aggregate";
import {
  formatCenterTokens,
  formatCost,
  formatNumber,
  formatSlicePercent,
} from "../lib/formatters";
import { UsageBucket } from "../lib/types";

type MetricMode = "tokens" | "cost";

export function DistributionGrid() {
  const state = useAppState();

  const filtered = useMemo(
    () => filterBuckets(state.buckets, state.filters, state.timeRange),
    [state.buckets, state.filters, state.timeRange],
  );

  return (
    <div className="grid grid-cols-2 items-stretch gap-[10px]">
      <DonutCard title="终端分布" Icon={Monitor} buckets={filtered} keyFn={(b) => b.hostname} />
      <DonutCard title="工具分布" Icon={SquareTerminal} buckets={filtered} keyFn={(b) => b.source} />
      <DonutCard title="模型分布" Icon={Cpu} buckets={filtered} keyFn={(b) => b.model} />
      <DonutCard title="项目分布" Icon={Folder} buckets={filtered} keyFn={(b) => b.project} />
    </div>
  );
}

function DonutCard({
  title,
  Icon,
  buckets,
  keyFn,
}: {
  title: string;
  Icon: typeof Monitor;
  buckets: UsageBucket[];
  keyFn: (b: UsageBucket) => string;
}) {
  const [mode, setMode] = useState<MetricMode>("tokens");
  const slices = useMemo(() => aggregateSlices(buckets, keyFn), [buckets, keyFn]);

  const total =
    mode === "tokens"
      ? slices.reduce((acc, s) => acc + s.tokens, 0)
      : slices.reduce((acc, s) => acc + s.cost, 0);

  return (
    <div className="flex flex-col gap-2.5 rounded-card border border-card-border bg-card p-[14px]">
      <div className="flex items-center">
        <div className="flex min-w-0 grow items-center gap-[5px]">
          <Icon size={11} color="#808080" className="shrink-0" />
          <span className="truncate text-[13px] font-medium text-t-secondary">{title}</span>
        </div>
        <MetricToggle mode={mode} setMode={setMode} />
      </div>

      {slices.length === 0 || total === 0 ? (
        <div className="flex h-20 items-center justify-center text-xs text-t-tertiary">
          暂无数据
        </div>
      ) : (
        <div className="flex flex-col items-center gap-3">
          <Donut slices={slices} mode={mode} total={total} />
          <div className="flex w-full flex-col gap-1.5">
            {slices.map((slice) => (
              <div key={slice.label} className="flex items-center gap-1.5">
                <span
                  className="h-[7px] w-[7px] shrink-0 rounded-full"
                  style={{ background: slice.color }}
                />
                <span className="min-w-0 grow truncate text-[11px]" style={{ color: "#B3B3B3" }}>
                  {slice.label}
                </span>
                <span className="shrink-0 font-mono text-[11px]" style={{ color: "#8C8C8C" }}>
                  {mode === "tokens" ? formatNumber(slice.tokens) : formatCost(slice.cost)}
                </span>
                <span
                  className="w-[42px] shrink-0 text-right font-mono text-[11px] text-t-tertiary"
                >
                  {formatSlicePercent(mode === "tokens" ? slice.tokens : slice.cost, total)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function MetricToggle({ mode, setMode }: { mode: MetricMode; setMode: (m: MetricMode) => void }) {
  const btn = (m: MetricMode, label: string) => {
    const active = mode === m;
    return (
      <button
        onClick={() => setMode(m)}
        className="rounded-full px-[9px] py-[3px] text-[11px]"
        style={{
          background: active ? "#474747" : "transparent",
          color: active ? "#FFFFFF" : "#808080",
          fontWeight: active ? 500 : 400,
        }}
      >
        {label}
      </button>
    );
  };
  return (
    <div className="flex shrink-0 rounded-full p-0.5" style={{ background: "#292929", gap: 2 }}>
      {btn("tokens", "Token")}
      {btn("cost", "费用")}
    </div>
  );
}

const DONUT_SIZE = 90;
const STROKE_WIDTH = 11;

function Donut({
  slices,
  mode,
  total,
}: {
  slices: SliceData[];
  mode: MetricMode;
  total: number;
}) {
  const r = DONUT_SIZE / 2 - STROKE_WIDTH / 2;
  const c = DONUT_SIZE / 2;
  const circumference = 2 * Math.PI * r;

  // stroke-dasharray segments starting at -90° (12 o'clock), clockwise.
  let acc = 0;
  const segments = slices.map((slice) => {
    const value = mode === "tokens" ? slice.tokens : slice.cost;
    const fraction = total > 0 ? value / total : 0;
    const seg = { slice, offset: acc, fraction };
    acc += fraction;
    return seg;
  });

  const centerLabel = mode === "tokens" ? formatCenterTokens(total) : formatCost(total);

  return (
    <div className="relative" style={{ width: DONUT_SIZE, height: DONUT_SIZE }}>
      <svg width={DONUT_SIZE} height={DONUT_SIZE}>
        <circle cx={c} cy={c} r={r} fill="none" stroke="#292929" strokeWidth={STROKE_WIDTH} />
        {segments.map(({ slice, offset, fraction }) => (
          <circle
            key={slice.label}
            cx={c}
            cy={c}
            r={r}
            fill="none"
            stroke={slice.color}
            strokeWidth={STROKE_WIDTH}
            strokeDasharray={`${fraction * circumference} ${circumference}`}
            strokeDashoffset={-offset * circumference}
            transform={`rotate(-90 ${c} ${c})`}
          />
        ))}
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center gap-px">
        <span className="text-[9px]" style={{ color: "#737373" }}>
          {mode === "tokens" ? "Tokens" : "预估"}
        </span>
        <span className="max-w-[52px] truncate font-mono text-xs font-bold text-white">
          {centerLabel}
        </span>
      </div>
    </div>
  );
}
