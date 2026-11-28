// Subscription quota selector and provider-neutral cards.

import { ReactNode, useCallback, useLayoutEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, ChevronUp } from "lucide-react";
import { createPortal } from "react-dom";

import { AppStateValue, useAppState } from "../state/AppStateContext";
import { api } from "../lib/api";
import {
  ProviderRateLimit,
  RateLimitMeter,
  RateLimitMeterPart,
  RateLimitProvider,
  RateLimitWindow,
} from "../lib/types";
import {
  QuotaTooltipLine,
  canonicalQuotaMeters,
  codexFiveHourPlaceholder,
  providerLabel,
  quotaAuthorizationText,
  quotaEmptyStateText,
  quotaProvenanceText,
  quotaRetryableText,
  quotaSignInText,
  quotaTooltipLines,
  quotaTooltipTitle,
} from "../lib/quotaProducts";
import { elapsedPercent, utilizationColor } from "../lib/aggregate";
import { formatPercent } from "../lib/formatters";
import { MiddleTruncateLabel } from "./MiddleTruncateLabel";
import { ProviderIcon } from "./ProviderIcon";
import { QuotaTabStrip } from "./QuotaTabStrip";

/**
 * Two cards per screen: each card is half the row minus half the 8 px gap, so
 * the pair always fills the content box. At the default (and minimum) window
 * width that is (520 − 2×16 padding − 8) / 2 = 240 px, the size the meters and
 * labels were laid out for; a wider window widens the cards instead of leaving
 * a gap on the right. A third product scrolls rather than squeezing the row.
 */
const CARD_GAP = 8;
export const QUOTA_CARD_BASIS = `calc((100% - ${CARD_GAP}px) / 2)`;

/** Scroll distance from one card to the next in a row `rowWidth` px wide. */
export function quotaCardStep(rowWidth: number): number {
  return (rowWidth + CARD_GAP) / 2;
}

export function RateLimitCards() {
  const state = useAppState();
  const selected = state.settings.selectedQuotaProductIds;
  const tabOrder = state.quotaTabOrder;
  // Only enabled products own a card; the strip still lists the whole catalog.
  const cardProviders = tabOrder.filter((provider) => selected.includes(provider));
  const scrollerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef({
    active: false,
    pointerId: -1,
    startX: 0,
    startScrollLeft: 0,
    moved: false,
  });
  const suppressClickRef = useRef(false);
  const [dragging, setDragging] = useState(false);
  const [activeProvider, setActiveProvider] = useState<RateLimitProvider>(() => {
    const stored = state.settings.quotaSelectedTabId;
    if (stored && cardProviders.includes(stored)) return stored;
    return cardProviders[0] ?? tabOrder[0] ?? "codex";
  });

  /** The card nearest the leading edge is the one on screen; the strip follows
   *  it and the choice is persisted, so the row reopens where the user left it. */
  const syncActiveFromScroll = useCallback(() => {
    const scroller = scrollerRef.current;
    if (!scroller || cardProviders.length === 0) return;
    const index = Math.min(
      cardProviders.length - 1,
      Math.max(0, Math.round(scroller.scrollLeft / quotaCardStep(scroller.clientWidth))),
    );
    const provider = cardProviders[index];
    setActiveProvider((current) => (current === provider ? current : provider));
    state.selectQuotaTab(provider);
  }, [cardProviders, state]);

  const selectFromTab = useCallback(
    (provider: RateLimitProvider) => {
      setActiveProvider(provider);
      state.selectQuotaTab(provider);
      const index = cardProviders.indexOf(provider);
      if (index < 0) return;
      const scroller = scrollerRef.current;
      if (!scroller) return;
      scroller.scrollTo({ left: index * quotaCardStep(scroller.clientWidth), behavior: "smooth" });
    },
    [cardProviders, state],
  );

  // Restore the last-viewed product as the leading card. Keyed on the row's
  // membership/order rather than on `activeProvider`: keying on the highlight
  // would fight the user's own scrolling.
  const rowSignature = cardProviders.join(",");
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const index = cardProviders.indexOf(activeProvider);
    if (index <= 0) return;
    scroller.scrollTo({ left: index * quotaCardStep(scroller.clientWidth) });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- see comment above
  }, [rowSignature]);

  const onCardsWheel = (event: React.WheelEvent<HTMLDivElement>) => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
    const maxScrollLeft = scroller.scrollWidth - scroller.clientWidth;
    const canMove = delta < 0 ? scroller.scrollLeft > 0 : scroller.scrollLeft < maxScrollLeft;
    if (!canMove) return;
    event.preventDefault();
    scroller.scrollLeft += delta;
  };

  const onCardsPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, a, input, select, textarea, [role='button']")) return;
    const scroller = scrollerRef.current;
    if (!scroller || scroller.scrollWidth <= scroller.clientWidth) return;
    dragRef.current = {
      active: true,
      pointerId: event.pointerId,
      startX: event.clientX,
      startScrollLeft: scroller.scrollLeft,
      moved: false,
    };
    scroller.setPointerCapture(event.pointerId);
  };

  const onCardsPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag.active || drag.pointerId !== event.pointerId) return;
    const distance = event.clientX - drag.startX;
    if (!drag.moved && Math.abs(distance) < 4) return;
    drag.moved = true;
    setDragging(true);
    event.preventDefault();
    event.currentTarget.scrollLeft = drag.startScrollLeft - distance;
  };

  const finishCardsDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag.active || drag.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    drag.active = false;
    setDragging(false);
    if (drag.moved) {
      suppressClickRef.current = true;
      window.setTimeout(() => {
        suppressClickRef.current = false;
      }, 0);
    }
  };

  const snapshot = (provider: RateLimitProvider): ProviderRateLimit =>
    state.rateLimits.find((item) => item.provider === provider) ?? {
      provider,
      status: { kind: "noData" },
    };

  return (
    <section className="flex flex-col gap-2">
      <QuotaTabStrip activeProvider={activeProvider} onSelect={selectFromTab} />

      {/* One card per *enabled* product, in strip order, inside a horizontal
          scroller (two per screen). A product whose monitoring is off has no
          card at all — its grey tab leads to Settings — so an all-off selection
          leaves the section at the icon row instead of a row of placeholders. */}
      {cardProviders.length === 0 ? null : (
        <div
          ref={scrollerRef}
          aria-label="已启用的订阅配额产品"
          className={`no-scrollbar flex items-stretch gap-2 overflow-x-auto ${
            cardProviders.length > 2 ? (dragging ? "cursor-grabbing" : "cursor-grab") : ""
          }`}
          style={{ scrollSnapType: dragging ? "none" : "x mandatory", touchAction: "pan-y" }}
          onScroll={syncActiveFromScroll}
          onWheel={onCardsWheel}
          onPointerDown={onCardsPointerDown}
          onPointerMove={onCardsPointerMove}
          onPointerUp={finishCardsDrag}
          onPointerCancel={finishCardsDrag}
          onClickCapture={(event) => {
            if (!suppressClickRef.current) return;
            event.preventDefault();
            event.stopPropagation();
          }}
          onDragStart={(event) => event.preventDefault()}
        >
          {cardProviders.map((provider) => (
            <div
              key={provider}
              className="shrink-0"
              style={{ flexBasis: QUOTA_CARD_BASIS, scrollSnapAlign: "start" }}
            >
              <ProviderCard snapshot={snapshot(provider)} />
            </div>
          ))}
        </div>
      )}

      {state.quotaSelectionError && (
        <div className="text-[11px] text-red-400">{state.quotaSelectionError}</div>
      )}
    </section>
  );
}

/** macOS `Color(red: 0.95, green: 0.72, blue: 0.25)`. */
const RESET_CREDITS_COLOR = "#F2B840";
const ROW_HEIGHT = 16;
const ROW_SPACING = 6;
/** Offset between the hovered row and its floating tooltip, plus the minimum
 *  distance kept from the window edge when the tooltip has to be nudged back. */
const TOOLTIP_GAP = 6;
const TOOLTIP_MARGIN = 8;

type RowItem =
  | { kind: "live"; label: string; window: RateLimitWindow; breakdown?: RateLimitMeterPart[] }
  | { kind: "placeholder"; label: string; message: string };

function meterWindow(meter: RateLimitMeter): RateLimitWindow {
  return {
    utilization: meter.utilization,
    resetsAt: meter.resetsAt,
    windowDuration: meter.windowDuration,
  };
}

export function ProviderCard({ snapshot }: { snapshot: ProviderRateLimit }) {
  const state = useAppState();
  const name = providerLabel(snapshot.provider, state.quotaProducts);
  const provenance = quotaProvenanceText(snapshot);
  const fiveHourPlaceholder = codexFiveHourPlaceholder(snapshot);

  const rows: RowItem[] = canonicalQuotaMeters(snapshot.meters ?? []).map((meter) => ({
    kind: "live",
    label: meter.label,
    window: meterWindow(meter),
    breakdown: meter.breakdown,
  }));
  if (rows.length === 0) {
    if (snapshot.fiveHour) {
      rows.push({ kind: "live", label: "5h", window: snapshot.fiveHour });
    } else if (fiveHourPlaceholder) {
      rows.push({ kind: "placeholder", label: "5h", message: fiveHourPlaceholder });
    }
    if (snapshot.sevenDay) rows.push({ kind: "live", label: "7d", window: snapshot.sevenDay });
  }
  for (const meter of snapshot.modelScopedMeters ?? []) {
    rows.push({ kind: "live", label: meter.label, window: meterWindow(meter) });
  }
  // Cards stay compact even when a provider exposes model-specific or
  // pay-as-you-go meters: the first three windows are shown and any remainder
  // folds behind a clickable line (mirrors RateLimitCardView.compactMeterLimit,
  // which covers Codex 5h/7d and OpenCode Go 5h/Weekly/Monthly outright).
  const [expanded, setExpanded] = useState(false);
  const compactMeterLimit = 3;
  const visibleRows = expanded ? rows : rows.slice(0, compactMeterLimit);
  const foldedMeterCount = Math.max(0, rows.length - compactMeterLimit);
  // The CLI asks for the app's own Cursor sign-in: the whole card opens the
  // Cursor row in Settings (no retry: reading again changes nothing).
  const signIn = snapshot.status.kind === "noData" && !state.isRefreshingRateLimits ? snapshot.signIn : null;
  const openSignIn = () => void api.openSettingsWindow("cursorLogin");

  return (
    <div
      className={`flex h-full min-w-0 flex-col gap-2.5 rounded-card border border-card-border bg-card px-3 py-[11px] ${
        signIn ? "cursor-pointer transition-colors hover:bg-white/[0.06]" : ""
      }`}
      {...(signIn
        ? {
            role: "button",
            tabIndex: 0,
            "aria-label": quotaSignInText(signIn),
            onClick: openSignIn,
            onKeyDown: (event: React.KeyboardEvent) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                openSignIn();
              }
            },
          }
        : {})}
    >
      <div className="flex items-center gap-1.5">
        <ProviderIcon provider={snapshot.provider} />
        <span className="truncate text-[13px] font-semibold text-white">{name}</span>
        {snapshot.resetCreditsCount != null && snapshot.resetCreditsCount > 0 && (
          // Amber capsule beside the name (macOS `RateLimitCardView.header`).
          <span
            className="shrink-0 whitespace-nowrap rounded-full px-1.5 py-0.5 text-[9px] font-medium leading-none"
            style={{ color: RESET_CREDITS_COLOR, background: "rgba(242,184,64,0.12)" }}
          >
            重置券 ×{snapshot.resetCreditsCount}
          </span>
        )}
        <div className="grow" />
        {state.isRefreshingRateLimits && <div className="spinner h-3 w-3 shrink-0" />}
        {snapshot.planLabel && (
          // One line, middle-truncated with the full name on hover (macOS
          // `.lineLimit(1).truncationMode(.middle).help(label)`). Wide enough
          // for 「Business ProLite」 beside the provider name.
          <MiddleTruncateLabel
            label={snapshot.planLabel}
            className="max-w-[120px] whitespace-nowrap rounded-full px-[7px] py-0.5 text-[10px] font-medium"
            style={{ background: "rgba(255,255,255,0.16)", color: "#8C8C8C" }}
          />
        )}
      </div>

      {snapshot.status.kind === "ok" && <QuotaRows rows={visibleRows} />}
      {snapshot.status.kind === "disabled" && <MessageContent text="订阅配额未启用" />}
      {signIn ? (
        <span className="flex items-center gap-1 text-[11px] leading-snug text-t-muted">
          <span className="min-w-0 grow">{quotaSignInText(signIn)}</span>
          <ChevronRight size={12} className="shrink-0 text-neutral-500" />
        </span>
      ) : (
        snapshot.status.kind === "noData" && <QuietText text={emptyStateText(snapshot, state)} />
      )}
      {snapshot.status.kind === "unauthorized" && (
        <MessageContent text={quotaAuthorizationText(snapshot, state.settings.zCodeQuotaRegion)} />
      )}
      {snapshot.status.kind === "retryableError" && (
        <MessageContent text={quotaRetryableText(snapshot)} />
      )}
      {snapshot.status.kind === "error" && <MessageContent text={snapshot.status.message} />}
      {snapshot.status.kind === "ok" && foldedMeterCount > 0 && (
        <button
          type="button"
          className="flex w-fit items-center gap-1 text-[10px] text-neutral-500 hover:text-neutral-300"
          aria-expanded={expanded}
          title={expanded ? "收起多余的配额窗口" : `展开全部 ${rows.length} 个配额窗口`}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? "收起" : `另有 ${foldedMeterCount} 项`}
          {expanded ? <ChevronUp size={10} /> : <ChevronDown size={10} />}
        </button>
      )}
      {provenance && <div className="text-[10px] text-neutral-400">{provenance}</div>}
      {snapshot.status.kind === "ok" && <FreshnessNote snapshot={snapshot} />}
    </div>
  );
}

/** Local discovery result only — never a credential or network read. It
 *  separates "installed but nothing to show yet" from "not on this machine". */
function emptyStateText(snapshot: ProviderRateLimit, state: AppStateValue): string {
  if (state.isRefreshingRateLimits) return "正在读取订阅配额…";
  const detected =
    state.quotaProducts.find((item) => item.provider === snapshot.provider)?.isDetected === true;
  return quotaEmptyStateText(
    snapshot,
    detected,
    false,
    providerLabel(snapshot.provider, state.quotaProducts),
  );
}

function QuietText({ text }: { text: string }) {
  return <span className="text-[11px] leading-snug text-neutral-500">{text}</span>;
}

function QuotaRows({ rows }: { rows: RowItem[] }) {
  const [hovered, setHovered] = useState<{ index: number; anchor: HTMLElement } | null>(null);
  const hoveredRow = hovered ? rows[hovered.index] : null;
  const hoveredWindow = hoveredRow?.kind === "live" ? hoveredRow.window : null;

  return (
    <div className="relative flex flex-col" style={{ gap: ROW_SPACING }}>
      {rows.map((row, index) =>
        row.kind === "live" ? (
          <QuotaRow
            key={`${row.label}-${index}`}
            label={row.label}
            window={row.window}
            onHover={(anchor) => setHovered(anchor ? { index, anchor } : null)}
          />
        ) : (
          <EmptyQuotaRow key={`${row.label}-${index}`} label={row.label} message={row.message} />
        ),
      )}
      {rows.length === 0 && <span className="text-[11px] text-neutral-500">暂无订阅配额数据</span>}

      {hovered && hoveredRow?.kind === "live" && hoveredWindow && (
        <TooltipLayer anchor={hovered.anchor}>
          <Tooltip
            label={hoveredRow.label}
            window={hoveredWindow}
            breakdown={hoveredRow.breakdown}
          />
        </TooltipLayer>
      )}
    </div>
  );
}

function QuotaRow({
  label,
  window: quotaWindow,
  onHover,
}: {
  label: string;
  window: RateLimitWindow;
  /** Anchor is the whole row, so the tooltip lines up with the label column. */
  onHover: (anchor: HTMLElement | null) => void;
}) {
  const elapsed = elapsedPercent(quotaWindow);
  const rowRef = useRef<HTMLDivElement | null>(null);
  return (
    <div ref={rowRef} className="flex items-center gap-1.5" style={{ height: ROW_HEIGHT }}>
      <span
        className="w-10 shrink-0 truncate font-mono text-[11px] font-medium"
        style={{ color: "#999999" }}
        title={label}
      >
        {label}
      </span>
      <div
        className="flex min-w-0 grow flex-col justify-center gap-0.5"
        onMouseEnter={() => rowRef.current && onHover(rowRef.current)}
        onMouseLeave={() => onHover(null)}
      >
        <ProgressBar value={quotaWindow.utilization} height={6} />
        {elapsed != null && (
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
        style={{ color: utilizationColor(quotaWindow.utilization) }}
      >
        {formatPercent(quotaWindow.utilization)}
      </span>
    </div>
  );
}

function EmptyQuotaRow({ label, message }: { label: string; message: string }) {
  return (
    <div className="flex items-center gap-1.5" style={{ height: ROW_HEIGHT }}>
      <span className="w-10 shrink-0 truncate font-mono text-[11px]" style={{ color: "#666666" }}>
        {label}
      </span>
      <span className="min-w-0 grow truncate text-[11px] text-neutral-500">{message}</span>
    </div>
  );
}

/**
 * Same content and colours as macOS `TooltipView` (PARITY rule 7): a title,
 * then one dotted line per fact — Token 用量, the window's time, and each part
 * of a meter's breakdown (Cursor's own vs third-party models).
 */
function Tooltip({
  label,
  window: quotaWindow,
  breakdown,
}: {
  label: string;
  window: RateLimitWindow;
  breakdown?: RateLimitMeterPart[];
}) {
  return (
    <div
      className="flex flex-col gap-[5px] whitespace-nowrap rounded-[5px] bg-black px-2.5 py-2 text-[11px] shadow-[0_2px_5px_rgba(0,0,0,0.5)]"
      style={{ border: "0.5px solid #383838" }}
    >
      <span className="font-semibold text-white">{quotaTooltipTitle(label)}</span>
      {quotaTooltipLines(quotaWindow, breakdown).map((line, index) => (
        <TooltipLine key={`${line.kind}-${index}`} line={line} />
      ))}
    </div>
  );
}

function TooltipLine({ line }: { line: QuotaTooltipLine }) {
  // macOS Color(white:): 0.55 → #8C8C8C, 0.82 → #D1D1D1, 0.5 → #808080,
  // 0.3 → #4D4D4D.
  const usageColor = utilizationColor(line.utilization ?? 0);
  const dot = line.kind === "usage" ? usageColor : line.kind === "time" ? "#8C8C8C" : "#4D4D4D";
  const valueColor = line.kind === "usage" ? usageColor : line.muted ? "#808080" : "#D1D1D1";
  return (
    <span className="flex items-center gap-1.5">
      <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: dot }} />
      <span style={{ color: "#8C8C8C" }}>{line.label}</span>
      <span style={{ color: valueColor, fontWeight: line.kind === "usage" ? 500 : 400 }}>
        {line.value}
      </span>
    </span>
  );
}

/**
 * Renders the quota tooltip into `document.body` as a fixed-position layer.
 *
 * The cards sit inside a horizontal scroller, and a scroll container clips all
 * of its descendants (`overflow-x: auto` also forces `overflow-y: auto`), so an
 * in-card tooltip on the last row gets cut off at the card edge. A body portal
 * is outside every ancestor clip and stacking context, so the tooltip always
 * paints complete and on top, at any scroll offset. It follows the row when the
 * scroller or the panel moves, and flips above the row near the window bottom.
 */
function TooltipLayer({ anchor, children }: { anchor: HTMLElement; children: ReactNode }) {
  const layerRef = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    const place = () => {
      const layer = layerRef.current;
      if (!layer) return;
      const row = anchor.getBoundingClientRect();
      const { width, height } = layer.getBoundingClientRect();
      const viewportWidth = document.documentElement.clientWidth;
      const viewportHeight = document.documentElement.clientHeight;
      const maxLeft = Math.max(TOOLTIP_MARGIN, viewportWidth - width - TOOLTIP_MARGIN);
      const left = Math.min(Math.max(row.left, TOOLTIP_MARGIN), maxLeft);
      const below = row.bottom + TOOLTIP_GAP;
      const above = row.top - TOOLTIP_GAP - height;
      const top =
        below + height + TOOLTIP_MARGIN <= viewportHeight
          ? below
          : above >= TOOLTIP_MARGIN
            ? above
            : Math.max(TOOLTIP_MARGIN, viewportHeight - height - TOOLTIP_MARGIN);
      setPos((current) =>
        current && current.left === left && current.top === top ? current : { left, top },
      );
    };
    place();
    // Capture phase so the quota scroller (not only the window) is covered.
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [anchor]);

  return createPortal(
    <div
      ref={layerRef}
      className="pointer-events-none fixed z-[9999]"
      style={pos ? { left: pos.left, top: pos.top } : { left: 0, top: 0, visibility: "hidden" }}
    >
      {children}
    </div>,
    document.body,
  );
}

/**
 * When a Claude Desktop record was written: that source can lag the live
 * account, so the Windows card says how old it is (macOS shows no age line
 * for it). A live or cached read adds no age line — macOS dropped
 * 「数据截至 N 分钟前」 (maintainer, 2026-10-24: too noisy), and the reset
 * credits sit beside the name.
 */
function FreshnessNote({ snapshot }: { snapshot: ProviderRateLimit }) {
  if (!snapshot.sourceLabel || !snapshot.dataAsOf) return null;
  return (
    <div className="truncate text-[10px] text-neutral-500">
      记录时间 {new Date(snapshot.dataAsOf * 1000).toLocaleString()}
    </div>
  );
}

function MessageContent({ text }: { text: string }) {
  const state = useAppState();
  return (
    <div className="flex items-center gap-2">
      <span className="min-w-0 grow text-[11px] leading-snug text-t-muted">{text}</span>
      <button
        className="shrink-0 rounded-full px-2.5 py-[3px] text-[11px]"
        style={{ background: "rgba(255,255,255,0.16)", color: "#C7C7C7" }}
        onClick={() => void state.refreshRateLimits(true)}
      >
        重试
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
  const percent = Math.min(Math.max(value, 0), 100);
  return (
    <div className="w-full overflow-hidden rounded-full" style={{ height, background }}>
      <div
        className="h-full rounded-full"
        style={{ width: `${percent}%`, background: fill ?? utilizationColor(value) }}
      />
    </div>
  );
}
