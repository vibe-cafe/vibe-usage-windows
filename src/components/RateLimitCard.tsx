// Subscription quota cards — port of Views/RateLimitCardView.swift.

import { useState } from "react";
import { Info, SquareTerminal, Sparkles } from "lucide-react";
import { useAppState } from "../state/AppStateContext";
import { ProviderRateLimit, RateLimitProvider, RateLimitWindow } from "../lib/types";
import { elapsedPercent, utilizationColor } from "../lib/aggregate";
import { formatPercent, formatTimeUntil } from "../lib/formatters";
import codexIcon from "../assets/codex-icon.png";
import claudeIcon from "../assets/claude-icon.png";

export function RateLimitCards() {
  const { rateLimits, settings } = useAppState();

  const snapshot = (provider: RateLimitProvider): ProviderRateLimit =>
    rateLimits.find((r) => r.provider === provider) ?? { provider, status: { kind: "noData" } };

  const codex = snapshot("codex");
  const claude = snapshot("claudeCode");
  // Match macOS: provider toggles own visibility; enabled Claude keeps an
  // actionable waiting card until its first statusline capture arrives.
  const showCodex = settings.codexRateLimitEnabled && codex.status.kind !== "noData";
  const showClaude = settings.claudeRateLimitEnabled;

  if (showCodex && showClaude) {
    return (
      <div className="grid grid-cols-2 items-stretch gap-2">
        <ProviderCard snapshot={codex} />
        <ProviderCard snapshot={claude} />
      </div>
    );
  }
  if (showCodex) return <ProviderCard snapshot={codex} />;
  if (showClaude) return <ProviderCard snapshot={claude} />;
  if (settings.codexRateLimitEnabled || settings.claudeRateLimitEnabled) return <NoticeBar />;
  return null;
}

/** Single-line whisper when neither provider has any data. */
function NoticeBar() {
  return (
    <div className="flex items-center gap-1.5" style={{ color: "#666666" }}>
      <Info size={10} />
      <span className="text-[11px]">支持 Codex / Claude 订阅配额监控</span>
    </div>
  );
}

const ROW_HEIGHT = 16;
const ROW_SPACING = 6;

type RowItem =
  | { kind: "live"; label: string; window: RateLimitWindow }
  | { kind: "placeholder"; label: string; message: string };

function ProviderCard({ snapshot }: { snapshot: ProviderRateLimit }) {
  const [hoveredLabel, setHoveredLabel] = useState<string | null>(null);
  const { settings } = useAppState();

  const displayName = snapshot.provider === "codex" ? "Codex" : "Claude";

  // Paid Codex plans always reserve the 5h slot (placeholder when expired).
  const plan = snapshot.planLabel?.toLowerCase();
  const expectsFiveHour =
    snapshot.provider === "codex" && (plan === "plus" || plan === "pro" || plan === "business");

  const rows: RowItem[] = [];
  if (snapshot.fiveHour) {
    rows.push({ kind: "live", label: "5h", window: snapshot.fiveHour });
  } else if (expectsFiveHour) {
    rows.push({ kind: "placeholder", label: "5h", message: "近 5 小时无活动" });
  }
  if (snapshot.sevenDay) rows.push({ kind: "live", label: "7d", window: snapshot.sevenDay });

  return (
    <div className="flex flex-col gap-2.5 rounded-card border border-card-border bg-card px-3 py-[11px]">
      {/* Header */}
      <div className="flex items-center gap-1.5">
        <ProviderIcon provider={snapshot.provider} />
        <span className="text-[13px] font-semibold text-white">{displayName}</span>
        <div className="grow" />
        {snapshot.planLabel && (
          <span
            className="rounded-full px-[7px] py-0.5 text-[10px] font-medium"
            style={{ background: "rgba(255,255,255,0.16)", color: "#8C8C8C" }}
          >
            {snapshot.planLabel}
          </span>
        )}
      </div>

      {/* Content by status */}
      {snapshot.status.kind === "ok" && (
        <QuotaRows rows={rows} hoveredLabel={hoveredLabel} setHoveredLabel={setHoveredLabel} />
      )}
      {snapshot.status.kind === "disabled" &&
        (snapshot.provider === "claudeCode" && settings.claudeRateLimitEnabled ? (
          <WaitingForClaudeContent />
        ) : (
          <DisabledContent />
        ))}
      {snapshot.status.kind === "noData" &&
        snapshot.provider === "claudeCode" &&
        settings.claudeRateLimitEnabled && <WaitingForClaudeContent />}
      {snapshot.status.kind === "unauthorized" && (
        <MessageContent text="未授权或登录已过期" action="重试" />
      )}
      {snapshot.status.kind === "error" && (
        <MessageContent text={snapshot.status.message} action="重试" />
      )}
    </div>
  );
}

function QuotaRows({
  rows,
  hoveredLabel,
  setHoveredLabel,
}: {
  rows: RowItem[];
  hoveredLabel: string | null;
  setHoveredLabel: (l: string | null) => void;
}) {
  const hoveredIndex = rows.findIndex((r) => r.label === hoveredLabel);
  const hoveredRow = hoveredIndex >= 0 ? rows[hoveredIndex] : null;
  const hoveredWindow = hoveredRow?.kind === "live" ? hoveredRow.window : null;

  return (
    <div className="relative flex flex-col" style={{ gap: ROW_SPACING }}>
      {rows.map((row) =>
        row.kind === "live" ? (
          <QuotaRow
            key={row.label}
            label={row.label}
            window={row.window}
            onHover={(h) => setHoveredLabel(h ? row.label : null)}
          />
        ) : (
          <EmptyQuotaRow key={row.label} label={row.label} message={row.message} />
        ),
      )}
      {rows.length === 0 && (
        <span className="text-[11px]" style={{ color: "#737373" }}>
          暂无订阅配额数据
        </span>
      )}

      {hoveredWindow && hoveredIndex >= 0 && (
        <div
          className="pointer-events-none absolute left-0 z-40"
          style={{ top: (hoveredIndex + 1) * ROW_HEIGHT + hoveredIndex * ROW_SPACING + 6 }}
        >
          <Tooltip label={rows[hoveredIndex].label} window={hoveredWindow} />
        </div>
      )}
    </div>
  );
}

function QuotaRow({
  label,
  window: win,
  onHover,
}: {
  label: string;
  window: RateLimitWindow;
  onHover: (hovering: boolean) => void;
}) {
  const elapsed = elapsedPercent(win);
  const hasElapsed = elapsed != null;

  return (
    <div className="flex items-center gap-1.5" style={{ height: ROW_HEIGHT }}>
      <span className="w-5 shrink-0 font-mono text-xs font-medium" style={{ color: "#999999" }}>
        {label}
      </span>

      <div
        className="flex min-w-0 grow flex-col justify-center gap-0.5"
        onMouseEnter={() => onHover(true)}
        onMouseLeave={() => onHover(false)}
      >
        <ProgressBar value={win.utilization} height={6} />
        {hasElapsed && (
          <ProgressBar
            value={elapsed}
            height={3}
            fill="rgba(255,255,255,0.42)"
            background="rgba(255,255,255,0.14)"
          />
        )}
      </div>

      <span
        className="w-9 shrink-0 text-right font-mono text-xs font-medium"
        style={{ color: utilizationColor(win.utilization) }}
      >
        {formatPercent(win.utilization)}
      </span>
    </div>
  );
}

function EmptyQuotaRow({ label, message }: { label: string; message: string }) {
  return (
    <div className="flex items-center gap-1.5" style={{ height: ROW_HEIGHT }}>
      <span className="w-5 shrink-0 font-mono text-xs font-medium" style={{ color: "#666666" }}>
        {label}
      </span>
      <span className="min-w-0 grow truncate text-[11px]" style={{ color: "#737373" }}>
        {message}
      </span>
    </div>
  );
}

function Tooltip({ label, window: win }: { label: string; window: RateLimitWindow }) {
  const title = label === "5h" ? "5 小时窗口" : label === "7d" ? "7 天窗口" : label;
  const elapsed = elapsedPercent(win);
  const remaining = win.resetsAt != null ? formatTimeUntil(new Date(win.resetsAt * 1000)) : null;
  const tokenColor = utilizationColor(win.utilization);

  const row = (dotColor: string, lbl: string, value: string, valueColor: string, medium = false) => (
    <div className="flex items-center gap-1.5 text-[11px]">
      <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: dotColor }} />
      <span style={{ color: "#8C8C8C" }}>{lbl}</span>
      <span style={{ color: valueColor, fontWeight: medium ? 500 : 400 }}>{value}</span>
    </div>
  );

  return (
    <div
      className="flex flex-col gap-[5px] whitespace-nowrap rounded-[5px] bg-black px-2.5 py-2 shadow-[0_2px_5px_rgba(0,0,0,0.5)]"
      style={{ border: "0.5px solid #383838" }}
    >
      <span className="text-[11px] font-semibold text-white">{title}</span>
      {row(tokenColor, "Token 用量", `已使用 ${formatPercent(win.utilization)}`, tokenColor, true)}
      {elapsed != null && remaining != null
        ? row(
            "#8C8C8C",
            "时间",
            `已过去 ${formatPercent(elapsed)} · 剩余 ${remaining}`,
            "rgba(255,255,255,0.82)",
          )
        : remaining != null
          ? row("#8C8C8C", "重置", `剩余 ${remaining}`, "rgba(255,255,255,0.82)")
          : row("#8C8C8C", "时间", "未知", "#808080")}
    </div>
  );
}

function DisabledContent() {
  const state = useAppState();
  const err = state.claudeRateLimitInstallError;
  return (
    <div className="flex items-center gap-2">
      <span
        className="min-w-0 grow truncate text-[11px]"
        style={{ color: err ? "#F04545" : "#808080" }}
      >
        {err ?? "读取 Claude 用量数据"}
      </span>
      <button
        className="shrink-0 rounded-full bg-white px-3 py-1 text-xs font-medium text-black"
        onClick={() => void state.enableClaudeRateLimit()}
      >
        启用
      </button>
    </div>
  );
}

function WaitingForClaudeContent() {
  const state = useAppState();
  return (
    <div className="flex items-center gap-2">
      <span className="min-w-0 grow text-[11px] leading-snug" style={{ color: "#808080" }}>
        已启用，使用 Claude Code 后会自动显示
      </span>
      <button
        className="shrink-0 rounded-full px-2.5 py-[3px] text-[11px]"
        style={{ background: "rgba(255,255,255,0.16)", color: "#C7C7C7" }}
        onClick={() => void state.refreshRateLimits(true)}
      >
        刷新
      </button>
    </div>
  );
}

function MessageContent({ text, action }: { text: string; action: string }) {
  const state = useAppState();
  return (
    <div className="flex items-center gap-2">
      <span className="min-w-0 grow truncate text-[11px] text-t-muted">{text}</span>
      <button
        className="shrink-0 rounded-full px-2.5 py-[3px] text-[11px]"
        style={{ background: "rgba(255,255,255,0.16)", color: "#C7C7C7" }}
        onClick={() => void state.refreshRateLimits(true)}
      >
        {action}
      </button>
    </div>
  );
}

function ProgressBar({
  value,
  height,
  fill,
  background = "rgba(255,255,255,0.18)",
}: {
  value: number;
  height: number;
  fill?: string;
  background?: string;
}) {
  const pct = Math.min(Math.max(value, 0), 100);
  return (
    <div className="w-full overflow-hidden rounded-full" style={{ height, background }}>
      <div
        className="h-full rounded-full"
        style={{ width: `${pct}%`, background: fill ?? utilizationColor(value) }}
      />
    </div>
  );
}

function ProviderIcon({ provider }: { provider: RateLimitProvider }) {
  const [failed, setFailed] = useState(false);
  if (failed) {
    return provider === "codex" ? (
      <SquareTerminal size={12} color="#999999" />
    ) : (
      <Sparkles size={12} color="#999999" />
    );
  }
  return (
    <img
      src={provider === "codex" ? codexIcon : claudeIcon}
      width={14}
      height={14}
      className="shrink-0"
      onError={() => setFailed(true)}
      alt=""
    />
  );
}
