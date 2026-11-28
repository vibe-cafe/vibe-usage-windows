// 订阅配额: one table of every product — monochrome mark and name, local
// status, the panel-card switch, a drag handle at the row's end (macOS
// `SettingsQuotaPage`, classic). Enabled products come first; the rest sit
// under 「未开启」. Both the order and the switches are the panel's own
// (`quotaProductOrder`, `selectedQuotaProductIds`): dragging here reorders the
// panel's tabs too, and turning a product on moves its row into the top group.
//
// A product that needs more than a switch (ZCode's key, Cursor's own sign-in)
// has a disclosure chevron at the row's start and opens into a rounded block
// under its row. Under the table, 「隐私保护」 holds the egress check's one
// setting, as on macOS.
//
// Rows are dragged with pointer events rather than HTML5 drag and drop, which
// a Tauri WebView on Windows only delivers with the window's file drop turned
// off; this keeps the row's switch and buttons clickable inside it.

import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { RefreshCw } from "lucide-react";
import { useSettings } from "../context";
import {
  Button,
  Card,
  CheckMark,
  DisclosureTriangle,
  IconLabel,
  MenuSelect,
  PageHeader,
  PageScroll,
  Row,
  RowLabel,
  Section,
  Segmented,
  Separator,
  Spacer,
  Spinner,
  StatusDot,
  Switch,
  TextField,
} from "../controls";
import { PROVIDER_ICON_OFF_COLOR, ProviderIcon } from "../../components/ProviderIcon";
import { EGRESS_MODES } from "../../lib/egressMode";
import { cursorLoginRow, CURSOR_LOGIN_WAITING } from "../../lib/cursorLogin";
import { providerLabel, type QuotaProductStatus } from "../../lib/quotaProducts";
import type { QuotaProduct, RateLimitProvider } from "../../lib/types";
import { SETTINGS_PAGE_TITLES, ZCODE_REGIONS, cursorRowConfigurable, quotaReorderTarget } from "../model";
import { T, TYPE } from "../theme";

/** Column metrics shared by the header and the rows: 状态 124 wide; names
 *  start 58 px in, after the disclosure slot and the 22 px mark; 40 px rows. */
const STATUS_WIDTH = 124;
const NAME_INSET = 58;
const ROW_HEIGHT = 40;
/** A pointer must travel this far before a press becomes a drag. */
const DRAG_THRESHOLD = 4;

interface DragState {
  provider: RateLimitProvider;
  pointerId: number;
  startY: number;
  /** Pointer offset inside the grabbed row, so the preview does not jump. */
  grabOffset: number;
  rowLeft: number;
  rowWidth: number;
  active: boolean;
  y: number;
}

export function QuotaPage() {
  const m = useSettings();
  const total = m.quotaProducts.length;
  return (
    <PageScroll bottomInset={8}>
      <PageHeader
        title={SETTINGS_PAGE_TITLES.quotas}
        detail={
          <span className="truncate" style={{ ...TYPE.body, color: T.secondaryText, fontVariantNumeric: "tabular-nums" }}>
            {`${m.enabledOrder.length} / ${total} 已开启 · 拖动排序`}
          </span>
        }
        trailing={
          <Button disabled={m.quotaBusy} title="重新检测本机产品" onClick={m.rediscoverQuotaProducts}>
            <IconLabel icon={<RefreshCw size={12} strokeWidth={2.2} />} title="重新检测" />
          </Button>
        }
      />
      <QuotaTable />
      {m.quotaError && (
        <span className="break-words" style={{ ...TYPE.caption, color: T.red, padding: "8px 2px 0" }}>
          {m.quotaError}
        </span>
      )}
      <Section title="隐私保护">
        <Row>
          <RowLabel title="没检测到代理时" />
          <Spacer />
          <MenuSelect
            label="没检测到代理时"
            value={m.egressMode}
            options={EGRESS_MODES}
            disabled={!m.settings}
            onChange={(value) => m.patchSettings({ egressMode: value })}
          />
        </Row>
      </Section>
    </PageScroll>
  );
}

function QuotaTable() {
  const m = useSettings();
  const rowLines = useRef(new Map<RateLimitProvider, HTMLDivElement>());
  const [drag, setDrag] = useState<DragState | null>(null);
  const [indicator, setIndicator] = useState<{ provider: RateLimitProvider; after: boolean } | null>(null);
  // A press that turned into a drag must not also toggle the row it ended on.
  const suppressClick = useRef(false);

  const productOf = (provider: RateLimitProvider): QuotaProduct =>
    m.quotaProducts.find((product) => product.provider === provider) ?? {
      provider,
      displayName: providerLabel(provider, m.quotaProducts),
      availability: "ready",
      isDetected: false,
    };

  const targetAt = (dragged: RateLimitProvider, y: number) => {
    for (const [provider, element] of rowLines.current) {
      const rect = element.getBoundingClientRect();
      if (y < rect.top || y >= rect.bottom) continue;
      const after = y > rect.top + rect.height / 2;
      const target = quotaReorderTarget(dragged, provider, after, m.enabledOrder, m.disabledOrder);
      return target.kind === "reject" ? null : { provider, after, target: target.provider };
    }
    return null;
  };

  const onPointerDown = (provider: RateLimitProvider, event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const rect = event.currentTarget.getBoundingClientRect();
    suppressClick.current = false;
    setDrag({
      provider,
      pointerId: event.pointerId,
      startY: event.clientY,
      grabOffset: event.clientY - rect.top,
      rowLeft: rect.left,
      rowWidth: rect.width,
      active: false,
      y: event.clientY,
    });
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const active = drag.active || Math.abs(event.clientY - drag.startY) > DRAG_THRESHOLD;
    if (!active) return;
    if (!drag.active) {
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // The pointer is already gone; the drag still follows its events.
      }
      suppressClick.current = true;
    }
    setDrag({ ...drag, active: true, y: event.clientY });
    const hit = targetAt(drag.provider, event.clientY);
    setIndicator(hit ? { provider: hit.provider, after: hit.after } : null);
  };

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>, drop: boolean) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    if (drag.active && drop) {
      const hit = targetAt(drag.provider, event.clientY);
      if (hit) m.moveQuotaProduct(drag.provider, hit.target);
    }
    setDrag(null);
    setIndicator(null);
  };

  useEffect(() => {
    if (!drag?.active) return;
    const cancel = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setDrag(null);
      setIndicator(null);
    };
    document.addEventListener("keydown", cancel);
    return () => document.removeEventListener("keydown", cancel);
  }, [drag?.active]);

  const rows = (group: RateLimitProvider[]) =>
    group.map((provider, index) => (
      <div key={provider} className="flex flex-col">
        {index > 0 && <Separator />}
        <QuotaRow
          product={productOf(provider)}
          status={m.quotaStatus(productOf(provider))}
          lineRef={(element) => {
            if (element) rowLines.current.set(provider, element);
            else rowLines.current.delete(provider);
          }}
          indicator={indicator?.provider === provider ? (indicator.after ? "bottom" : "top") : null}
          dragging={drag?.active === true && drag.provider === provider}
          onPointerDown={(event) => onPointerDown(provider, event)}
          onPointerMove={onPointerMove}
          onPointerUp={(event) => endDrag(event, true)}
          onPointerCancel={(event) => endDrag(event, false)}
          consumeClick={() => {
            const suppressed = suppressClick.current;
            suppressClick.current = false;
            return suppressed;
          }}
        />
      </div>
    ));

  return (
    <>
      <Card style={{ overflow: "hidden", cursor: drag?.active ? "grabbing" : undefined }}>
        <TableHeader />
        {rows(m.enabledOrder)}
        {m.disabledOrder.length > 0 && (
          <>
            <div
              className="flex items-center"
              style={{ ...TYPE.caption, color: T.secondaryText, minHeight: 26, padding: "0 14px", background: T.band }}
            >
              未开启
            </div>
            {rows(m.disabledOrder)}
          </>
        )}
      </Card>
      {drag?.active && (
        <DragPreview
          product={productOf(drag.provider)}
          status={m.quotaStatus(productOf(drag.provider))}
          on={m.enabledOrder.includes(drag.provider)}
          left={drag.rowLeft + 4}
          width={Math.min(440, drag.rowWidth - 8)}
          top={drag.y - drag.grabOffset}
        />
      )}
    </>
  );
}

/** 工具 · 状态 · 面板卡片, aligned with the row columns. */
function TableHeader() {
  return (
    <div
      className="flex items-center"
      style={{
        ...TYPE.caption,
        color: T.secondaryText,
        gap: 10,
        height: 28,
        paddingLeft: NAME_INSET,
        paddingRight: 12,
        boxShadow: `inset 0 -1px 0 ${T.line}`,
      }}
    >
      <span className="min-w-0 flex-1">工具</span>
      <span className="shrink-0" style={{ width: STATUS_WIDTH }}>
        状态
      </span>
      <span className="flex shrink-0 justify-end whitespace-nowrap" style={{ width: 30 }}>
        面板卡片
      </span>
      <span className="shrink-0" style={{ width: 10 }} />
    </div>
  );
}

function QuotaRow({
  product,
  status,
  lineRef,
  indicator,
  dragging,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onPointerCancel,
  consumeClick,
}: {
  product: QuotaProduct;
  status: QuotaProductStatus;
  lineRef: (element: HTMLDivElement | null) => void;
  indicator: "top" | "bottom" | null;
  dragging: boolean;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerCancel: (event: ReactPointerEvent<HTMLDivElement>) => void;
  consumeClick: () => boolean;
}) {
  const m = useSettings();
  const provider = product.provider;
  const on = m.enabledOrder.includes(provider);
  const configurable = provider === "zcode" || (provider === "cursor" && cursorRowConfigurable(m.cursorLogin));
  const expanded = configurable && m.expanded.has(provider);
  const wrapper = useRef<HTMLDivElement>(null);
  const flash = useFlash(provider, wrapper);

  return (
    <div ref={wrapper} className="flex flex-col">
      <div
        ref={lineRef}
        className="relative flex items-center"
        style={{
          gap: 10,
          height: ROW_HEIGHT,
          paddingLeft: 26,
          paddingRight: 12,
          touchAction: "none",
          cursor: dragging ? "grabbing" : undefined,
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
      >
        {/* The flash after arriving from the panel. */}
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-y-0"
          style={{
            left: 4,
            right: 4,
            borderRadius: 6,
            background: T.selection,
            opacity: flash.on ? 1 : 0,
            transition: flash.on ? "none" : "opacity 1.2s ease-out",
          }}
        />
        {configurable && (
          <button
            type="button"
            aria-expanded={expanded}
            aria-label={expanded ? `收起 ${product.displayName}` : `展开 ${product.displayName}`}
            title={expanded ? "收起" : "展开"}
            onPointerDown={(event) => event.stopPropagation()}
            onClick={() => m.toggleExpanded(provider)}
            className="settings-focus absolute inline-flex items-center justify-center"
            style={{ left: 6, width: 20, height: 20, top: (ROW_HEIGHT - 20) / 2 }}
          >
            <DisclosureTriangle open={expanded} />
          </button>
        )}
        <div
          className="relative flex min-w-0 flex-1 items-center"
          style={{ gap: 10 }}
          onClick={() => {
            if (consumeClick()) return;
            if (configurable) m.toggleExpanded(provider);
          }}
        >
          <ProviderIcon provider={provider} size={22} color={on ? T.text : PROVIDER_ICON_OFF_COLOR} />
          <span className="min-w-0 flex-1 truncate" style={{ ...TYPE.label, color: on ? T.text : T.secondaryText }}>
            {product.displayName}
          </span>
          <StatusCell status={status} on={on} />
        </div>
        <span className="relative inline-flex">
          <Switch
            label={product.displayName}
            checked={on}
            disabled={m.quotaBusy}
            onChange={(value) => m.toggleQuotaProduct(provider, value)}
          />
        </span>
        <DragHandle dimmed={!on} />
        {indicator && (
          <span
            aria-hidden="true"
            className="pointer-events-none absolute z-[2]"
            style={{
              left: 10,
              right: 10,
              height: 2,
              borderRadius: 1,
              background: T.ok,
              [indicator]: -1,
            }}
          />
        )}
      </div>
      {expanded && (
        <div style={{ padding: `0 ${T.rowTrailing}px 12px ${T.rowLeading}px` }}>
          {provider === "cursor" ? <CursorLoginBlock /> : <ZCodeConfigBlock />}
        </div>
      )}
    </div>
  );
}

/** Pulse the row once per arrival from the panel and bring it into view. */
function useFlash(provider: RateLimitProvider, row: React.RefObject<HTMLDivElement | null>) {
  const { flash } = useSettings();
  const [on, setOn] = useState(false);
  const request = flash?.provider === provider ? flash.request : null;
  useEffect(() => {
    if (request == null) return;
    row.current?.scrollIntoView({ block: "center", behavior: "smooth" });
    setOn(true);
    const timer = window.setTimeout(() => setOn(false), 250);
    return () => window.clearTimeout(timer);
  }, [request, row]);
  return { on };
}

/** Status in the text color while on, grey while off; only a problem is colored. */
function StatusCell({ status, on }: { status: QuotaProductStatus; on: boolean }) {
  const color =
    status.kind === "warning" ? T.amber : status.kind === "error" ? T.red : on ? T.text : T.secondaryText;
  return (
    <span className="flex shrink-0 items-center" style={{ width: STATUS_WIDTH, gap: 7 }}>
      <StatusDot kind={status.kind} dimmed={!on} />
      <span className="truncate" style={{ ...TYPE.body, color }} title={status.text}>
        {status.text}
      </span>
    </span>
  );
}

/** Six dots, always visible: the row can be dragged anywhere, the handle says so. */
function DragHandle({ dimmed }: { dimmed: boolean }) {
  return (
    <svg
      aria-hidden="true"
      width={10}
      height={16}
      viewBox="0 0 10 16"
      className="shrink-0"
      style={{ opacity: dimmed ? 0.38 : 0.75, cursor: "grab" }}
    >
      <title>拖动排序</title>
      {[2.6, 7.4].flatMap((x) => [3, 8, 13].map((y) => <circle key={`${x}-${y}`} cx={x} cy={y} r={1.2} fill={T.tertiaryText} />))}
    </svg>
  );
}

/** What follows the pointer while a row is dragged: the row, lifted. */
function DragPreview({
  product,
  status,
  on,
  left,
  width,
  top,
}: {
  product: QuotaProduct;
  status: QuotaProductStatus;
  on: boolean;
  left: number;
  width: number;
  top: number;
}) {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none fixed z-40 flex items-center"
      style={{
        left,
        top,
        width,
        height: ROW_HEIGHT,
        gap: 10,
        padding: "0 18px 0 20px",
        borderRadius: 9,
        background: T.raised,
        boxShadow: `inset 0 0 0 1px ${T.strongLine}, 0 8px 22px rgba(0,0,0,0.4)`,
      }}
    >
      <ProviderIcon provider={product.provider} size={22} color={T.text} />
      <span className="min-w-0 flex-1 truncate" style={{ ...TYPE.label, color: T.text }}>
        {product.displayName}
      </span>
      <StatusCell status={status} on={on} />
      <span
        className="relative shrink-0 rounded-full"
        style={{ width: 30, height: 18, background: on ? T.ok : "rgba(255,255,255,0.17)" }}
      >
        <span
          className="absolute top-[2px] rounded-full"
          style={{ width: 14, height: 14, left: on ? 14 : 2, background: "#F4F4F4" }}
        />
      </span>
    </div>
  );
}

/** The rounded block an expanded row opens into, a step lighter than the card. */
function ConfigBlock({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col" style={{ padding: 12, borderRadius: 10, background: T.configBlock }}>
      {children}
    </div>
  );
}

/**
 * ZCode's key on one line (maintainer, 2026-11-20: 「ZCode不要弄两行了，一行不行吗」):
 * region segments, the key field (or 「✓ Key 已保存」), then the buttons. The key
 * goes to Windows Credential Manager. Save never turns the switch on; remove
 * turns it off.
 */
function ZCodeConfigBlock() {
  const m = useSettings();
  const field = useRef<HTMLInputElement>(null);
  const showsSaved = m.zCodeConfigured && !m.zCodeReplacing;
  const region = ZCODE_REGIONS.find((item) => item.value === m.zCodeRegion) ?? ZCODE_REGIONS[0];
  useEffect(() => {
    if (m.zCodeReplacing) field.current?.focus();
  }, [m.zCodeReplacing]);
  return (
    <ConfigBlock>
      <div
        className="grid items-center"
        style={{ gridTemplateColumns: "auto minmax(0, 1fr) auto", columnGap: 8, rowGap: 5 }}
      >
        <Segmented
          label="ZCode 账号区域"
          options={ZCODE_REGIONS}
          value={m.zCodeRegion}
          disabled={m.quotaBusy || !m.settings}
          onChange={m.setZCodeRegion}
        />
        {showsSaved ? (
          <>
            <span className="flex min-w-0 items-center" style={{ gap: 6, paddingLeft: 4 }}>
              <CheckMark />
              <span className="truncate" style={{ ...TYPE.body, color: T.text }}>
                Key 已保存
              </span>
            </span>
            <span className="flex items-center" style={{ gap: 8 }}>
              <Button disabled={m.quotaBusy} onClick={m.beginReplacingZCodeKey}>
                更换
              </Button>
              <Button kind="danger" disabled={m.quotaBusy} onClick={m.removeZCodeKey}>
                移除
              </Button>
            </span>
          </>
        ) : (
          <>
            <TextField
              inputRef={field}
              prompt="粘贴 API Key"
              title={region.keyName}
              secure
              value={m.zCodeDraft}
              disabled={m.quotaBusy || !m.settings}
              onChange={m.setZCodeDraft}
              onSubmit={m.saveZCodeKey}
              onEscape={m.zCodeReplacing ? m.cancelReplacingZCodeKey : undefined}
            />
            <Button disabled={m.quotaBusy || !m.zCodeDraft.trim()} onClick={m.saveZCodeKey}>
              保存
            </Button>
          </>
        )}
        {m.zCodeError && (
          <span className="break-words" style={{ ...TYPE.caption, color: T.red, gridColumn: "2 / span 2" }}>
            {m.zCodeError}
          </span>
        )}
      </div>
    </ConfigBlock>
  );
}

/**
 * Cursor's own sign-in for Vibe Usage, on one line inside the same rounded
 * block: where it stands on the left, the button(s) on the right. Copy in
 * `lib/cursorLogin`, word for word the macOS app's. Signing out asks first.
 */
function CursorLoginBlock() {
  const m = useSettings();
  const status = m.cursorLogin;
  if (!status) return null;
  const row = cursorLoginRow(status);
  const error = status.state === "waiting" ? null : (m.cursorLoginError ?? row.error);
  return (
    <ConfigBlock>
      <div className="flex flex-col" style={{ gap: 8 }}>
        <div className="flex items-center" style={{ gap: 8 }}>
          <span className="flex min-w-0 flex-1 items-center" style={{ gap: status.state === "waiting" ? 8 : 6 }}>
            {status.state === "waiting" && <Spinner />}
            {status.state === "signedIn" && <CheckMark />}
            <span
              className={status.state === "waiting" ? "break-words" : "truncate"}
              title={row.text ?? undefined}
              style={{
                ...TYPE.body,
                color:
                  status.state === "signedOut"
                    ? T.secondaryText
                    : status.state === "expired"
                      ? T.amber
                      : T.text,
              }}
            >
              {status.state === "waiting" ? CURSOR_LOGIN_WAITING : row.text}
            </span>
          </span>
          {/* An ended sign-in: 退出 (nothing to revoke, no confirmation) and
              重新登录, both buttons like macOS. */}
          {row.secondary && <Button onClick={() => m.cursorLoginAction(row.secondary!.kind)}>{row.secondary.label}</Button>}
          {row.action && <Button onClick={() => m.cursorLoginAction(row.action!.kind)}>{row.action.label}</Button>}
        </div>
        {error && <span style={{ ...TYPE.caption, color: T.red }}>{error}</span>}
      </div>
    </ConfigBlock>
  );
}
