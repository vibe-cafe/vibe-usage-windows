// The 订阅配额 tab strip: one icon per product, the settings icon pinned to
// the trailing edge. Mirrors QuotaTabStripView.swift on macOS.
//
// The strip is the card row's index in both directions: clicking a tab scrolls
// the row to that product, and scrolling the row moves the highlight to the
// card that reached the leading edge — the strip never claims a product is on
// screen when it is not. Enabled products come first in the full-strength
// monochrome mark; products whose monitoring is off stay visible after them,
// dimmed to grey, and their tab opens Settings (there is no card to jump to).
// Tabs are draggable: the order is persisted (`quotaProductOrder`), and because enabled
// products render as their own group, a cross-group drop lands inside the
// dragged product's own group instead of breaking the "enabled first" rule.

import { useState } from "react";
import { Settings } from "lucide-react";
import { RateLimitProvider } from "../lib/types";
import { isQuotaPause, providerLabel, quotaTabShowsWarning } from "../lib/quotaProducts";
import { useAppState } from "../state/AppStateContext";
import { api } from "../lib/api";
import { ProviderIcon } from "./ProviderIcon";

/** Same amber as the 70–90% progress bar: "this product has a problem" reads
 *  like "this window is nearly full". */
const WARNING_COLOR = "#F59E0B";

export function QuotaTabStrip({
  activeProvider,
  onSelect,
}: {
  activeProvider: RateLimitProvider;
  onSelect: (provider: RateLimitProvider) => void;
}) {
  const state = useAppState();
  const [dragging, setDragging] = useState<RateLimitProvider | null>(null);
  const order = state.quotaTabOrder;

  const isEnabled = (provider: RateLimitProvider) =>
    state.settings.selectedQuotaProductIds.includes(provider);

  const drop = (target: RateLimitProvider | null) => {
    if (dragging && dragging !== target) state.moveQuotaProduct(dragging, target);
    setDragging(null);
  };

  return (
    <div
      role="tablist"
      aria-label="订阅配额产品"
      className="flex items-center gap-1.5"
      style={{ height: 30 }}
      onDragEnd={() => setDragging(null)}
    >
      {order.map((provider) => {
        const enabled = isEnabled(provider);
        const active = activeProvider === provider;
        const snapshot = state.rateLimits.find((item) => item.provider === provider);
        const warning = quotaTabShowsWarning(
          provider,
          state.settings.selectedQuotaProductIds,
          snapshot,
          state.isRefreshingRateLimits,
        );
        // A paused read keeps sound numbers: only the reading stopped.
        const warningText = isQuotaPause(snapshot?.liveFailure) ? " · 读取已暂停" : " · 数据异常";
        const help = `${providerLabel(provider, state.quotaProducts)}${enabled ? "" : " · 未启用"}${warning ? warningText : ""}`;
        return (
          <button
            key={provider}
            type="button"
            role="tab"
            aria-selected={active}
            draggable
            title={help}
            className="relative flex shrink-0 items-center justify-center rounded-[7px] focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1 focus-visible:outline-white/50"
            style={{
              width: 30,
              height: 30,
              background: active ? "#1C1C1C" : "transparent",
              boxShadow: active ? "inset 0 0 0 1px #444444" : undefined,
              // Off products keep their slot but dim: the mark itself
              // carries "on / off", so a greyed tab is a product the user
              // can look at (and enable) rather than one that is gone.
              opacity: enabled ? 1 : 0.45,
              cursor: dragging === provider ? "grabbing" : undefined,
            }}
            onClick={() => {
              // A grey tab has no card to jump to: monitoring is off, so the
              // useful destination is the place that turns it on.
              if (enabled) onSelect(provider);
              else void api.openSettingsWindow();
            }}
            onDragStart={(event) => {
              event.dataTransfer.effectAllowed = "move";
              event.dataTransfer.setData("text/plain", provider);
              setDragging(provider);
            }}
            onDragOver={(event) => {
              if (!dragging || dragging === provider) return;
              event.preventDefault();
              event.dataTransfer.dropEffect = "move";
            }}
            onDrop={(event) => {
              event.preventDefault();
              drop(provider);
            }}
          >
            <ProviderIcon provider={provider} size={20} />
            {warning && (
              <span
                aria-hidden="true"
                className="absolute right-0 top-0 h-1.5 w-1.5 rounded-full"
                style={{ background: WARNING_COLOR }}
              />
            )}
          </button>
        );
      })}

      {/* Dropping past the last icon moves the product to the end of its own
          group — dragging onto the final tab can only mean "before it". */}
      <div
        className="min-w-3 grow self-stretch"
        onDragOver={(event) => {
          if (!dragging) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
        }}
        onDrop={(event) => {
          event.preventDefault();
          drop(null);
        }}
      />

      <button
        type="button"
        aria-label="打开订阅配额设置"
        title="打开订阅配额设置"
        className="flex shrink-0 items-center justify-center rounded-[7px] text-neutral-400 hover:text-neutral-200 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1 focus-visible:outline-white/50"
        style={{ width: 30, height: 30 }}
        onClick={() => void api.openSettingsWindow()}
      >
        <Settings size={13} />
      </button>
    </div>
  );
}
