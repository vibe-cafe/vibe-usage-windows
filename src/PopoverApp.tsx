// Main popover container — port of Views/PopoverView.swift (520×620 panel).

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, onPanelWillHide } from "./lib/api";
import { useAppState } from "./state/AppStateContext";
import { HeaderBar } from "./components/HeaderBar";
import { OnboardingView } from "./components/OnboardingView";
import { RateLimitCards } from "./components/RateLimitCard";
import { FilterTags } from "./components/FilterTags";
import { SummaryCards } from "./components/SummaryCards";
import { TrendChart } from "./components/TrendChart";
import { DistributionGrid } from "./components/DistributionGrid";
import { FooterBar } from "./components/FooterBar";
import { Inbox } from "lucide-react";

type PanelAnim = "opening" | "open" | "closing";

export function PopoverApp() {
  const state = useAppState();
  const [anim, setAnim] = useState<PanelAnim>("opening");

  // Panel open/close animation driven by Rust show/hide events. The origin
  // matches the tray position (bottom taskbar → panel scales up from its
  // bottom-right corner, like the macOS panel falling out of the menu bar).
  useEffect(() => {
    const subs = [
      listen<{ origin?: string }>("panel-shown", (e) => {
        document.documentElement.style.setProperty(
          "--panel-origin",
          e.payload?.origin ?? "top right",
        );
        setAnim("opening");
      }),
      onPanelWillHide(() => setAnim("closing")),
    ];
    return () => {
      for (const p of subs) void p.then((un) => un());
    };
  }, []);

  useEffect(() => {
    if (anim === "opening") {
      const t = setTimeout(() => setAnim("open"), 230);
      return () => clearTimeout(t);
    }
    if (anim === "closing") {
      const t = setTimeout(() => void api.hidePanel(), 150);
      return () => clearTimeout(t);
    }
  }, [anim]);

  // ESC closes the panel (mirrors the local ESC event monitor).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setAnim("closing");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const animClass =
    anim === "opening" ? "panel-opening" : anim === "closing" ? "panel-closing" : "";

  return (
    <div
      className={`flex h-[620px] w-[520px] flex-col overflow-hidden rounded-panel bg-app ${animClass}`}
    >
      {!state.configured ? <OnboardingView /> : <DashboardView />}
    </div>
  );
}

function DashboardView() {
  const state = useAppState();

  return (
    <div className="flex h-full flex-col">
      <div className="px-4 pb-2 pt-3">
        <HeaderBar />
      </div>
      <div className="h-px shrink-0 bg-card-border" />

      <div className="no-scrollbar h-[560px] grow overflow-y-auto">
        <div className="flex flex-col gap-[14px] p-4">
          {state.isInitialDataLoad || (!state.hasLoadedUsageData && state.buckets.length === 0) ? (
            <>
              <RateLimitCards />
              <div className="my-0.5 h-px bg-card-border" />
              <LoadingDashboard />
            </>
          ) : !state.hasAnyData ? (
            <>
              <RateLimitCards />
              <EmptyState />
            </>
          ) : (
            <DashboardContent />
          )}
        </div>
      </div>

      <div className="h-px shrink-0 bg-card-border" />
      <div className="px-4 py-2">
        <FooterBar />
      </div>
    </div>
  );
}

function DashboardContent() {
  const state = useAppState();
  return (
    <div className="relative">
      <div
        className="flex flex-col gap-[14px] transition-opacity duration-200"
        style={{ opacity: state.isRefreshingData ? 0.72 : 1 }}
      >
        <RateLimitCards />
        <div className="my-0.5 h-px bg-card-border" />
        <div className="relative z-20">
          <FilterTags />
        </div>
        <SummaryCards />
        <TrendChart />
        <DistributionGrid />
      </div>
      {state.isRefreshingData && (
        <div className="absolute inset-x-0 top-0 z-30 flex justify-center pt-[90px]">
          <LoadingPill />
        </div>
      )}
    </div>
  );
}

/** Floating "加载中" capsule (ultraThinMaterial equivalent). */
export function LoadingPill() {
  return (
    <div
      className="flex items-center gap-2 rounded-full border border-white/10 px-3 py-2 shadow-[0_5px_10px_rgba(0,0,0,0.28)] backdrop-blur-md"
      style={{ background: "rgba(40,40,40,0.6)" }}
    >
      <div className="spinner h-3.5 w-3.5" />
      <span className="text-xs font-medium" style={{ color: "rgba(255,255,255,0.66)" }}>
        加载中
      </span>
    </div>
  );
}

function LoadingDashboard() {
  return (
    <div className="relative flex flex-col gap-[14px]">
      <div className="flex gap-2">
        {[0, 1, 2, 3].map((i) => (
          <SkeletonBlock key={i} height={70} className="min-w-0 flex-1" />
        ))}
      </div>
      <SkeletonBlock height={238} />
      <div className="grid grid-cols-2 gap-[10px]">
        {[0, 1, 2, 3].map((i) => (
          <SkeletonBlock key={i} height={190} />
        ))}
      </div>
      <div className="absolute inset-x-0 top-[90px] flex justify-center">
        <LoadingPill />
      </div>
    </div>
  );
}

function SkeletonBlock({ height, className = "" }: { height: number; className?: string }) {
  return (
    <div
      className={`skeleton rounded-card border border-card-border bg-card ${className}`}
      style={{ height }}
    />
  );
}

function EmptyState() {
  return (
    <div className="flex h-[200px] w-full flex-col items-center justify-center gap-3">
      <Inbox size={32} color="#4D4D4D" strokeWidth={1.5} />
      <div className="text-[15px] font-medium text-t-muted">暂无数据</div>
      <div className="text-[13px] text-t-tertiary">使用 AI 编程工具后数据将自动同步</div>
    </div>
  );
}
