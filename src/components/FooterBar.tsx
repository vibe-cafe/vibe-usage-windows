// Footer — port of PopoverView.footerBar.
// Note: 关闭 quits the app, exactly like the macOS footer's power button
// (NSApplication.terminate) — it is NOT "close panel".

import { AlertCircle, ArrowUpCircle, CheckCircle2, Power, RotateCw } from "lucide-react";
import { api } from "../lib/api";
import { useAppState } from "../state/AppStateContext";
import { formatRelativeTime } from "../lib/formatters";

export function FooterBar() {
  const state = useAppState();
  const { syncState, updateInfo } = state;
  const syncing = syncState.status === "syncing";

  return (
    <div className="flex items-center">
      {/* Sync status */}
      <div className="flex min-w-0 items-center gap-1.5">
        {syncing ? (
          <div className="spinner h-3 w-3 shrink-0" />
        ) : syncState.status === "error" ? (
          <AlertCircle size={12} color="#EF4444" fill="#EF4444" stroke="#0A0A0A" className="shrink-0" />
        ) : (
          <CheckCircle2 size={12} color="#33CC80" fill="#33CC80" stroke="#0A0A0A" className="shrink-0" />
        )}
        <span className="min-w-0 truncate text-[11px] text-t-tertiary">
          {syncing
            ? "同步中..."
            : syncState.status === "error"
              ? (syncState.message ?? "同步失败")
              : syncState.lastSyncAt
                ? `上次同步: ${formatRelativeTime(new Date(syncState.lastSyncAt))}`
                : "就绪"}
        </span>
      </div>

      <div className="grow" />

      {/* App update */}
      {updateInfo && (
        <button
          className="mr-3 flex shrink-0 items-center gap-1 text-[11px] font-medium text-link"
          onClick={() => void api.installUpdate()}
        >
          <ArrowUpCircle size={12} />
          发现更新
        </button>
      )}

      {/* Refresh */}
      <button
        className="flex shrink-0 items-center gap-1 text-[11px] text-t-muted disabled:opacity-50"
        disabled={syncing}
        onClick={() => {
          // CLI sync upload + rate-limit refresh are independent — fire both.
          void state.triggerSync();
          void state.refreshRateLimits(true);
        }}
      >
        <RotateCw size={12} />
        更新数据
      </button>

      {/* Quit */}
      <button
        className="ml-3 flex shrink-0 items-center gap-1 text-[11px] text-t-muted"
        onClick={() => void api.quitApp()}
      >
        <Power size={12} />
        关闭
      </button>
    </div>
  );
}
