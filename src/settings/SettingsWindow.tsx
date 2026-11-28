// The window's content: the 168 px sidebar, a 1 px rule, the chosen page
// (macOS `SettingsWindowContent` + `SettingsSidebar`, classic theme). Each
// page scrolls on its own when it is taller than the window; the sidebar
// never moves. Windows draws its own title bar above this, so the sidebar
// starts under it instead of leaving room for macOS's traffic lights.

import type { ReactNode } from "react";
import { Folder, Gauge, Info, PanelBottom, RefreshCw } from "lucide-react";
import { useSettings } from "./context";
import { AppIcon, StatusDot } from "./controls";
import { SETTINGS_PAGES, SETTINGS_PAGE_TITLES, type SettingsPage, syncSidebarDot } from "./model";
import { T, TYPE } from "./theme";
import { GeneralPage } from "./pages/GeneralPage";
import { QuotaPage } from "./pages/QuotaPage";
import { SyncPage } from "./pages/SyncPage";
import { DirectoriesPage } from "./pages/DirectoriesPage";
import { AboutPage } from "./pages/AboutPage";

/** Lucide stand-ins for the macOS SF Symbols; 通用 uses a screen with a bar
 *  along the bottom — the taskbar the tray lives in — for macOS's menu bar. */
const PAGE_ICONS: Record<SettingsPage, ReactNode> = {
  general: <PanelBottom size={15} strokeWidth={1.6} />,
  quotas: <Gauge size={15} strokeWidth={1.6} />,
  sync: <RefreshCw size={14} strokeWidth={1.7} />,
  directories: <Folder size={15} strokeWidth={1.6} />,
  about: <Info size={15} strokeWidth={1.6} />,
};

export function SettingsWindow() {
  const { page } = useSettings();
  return (
    <div
      className="flex h-screen w-screen overflow-hidden"
      style={{ background: T.background, color: T.text, fontFamily: T.font }}
    >
      <Sidebar />
      <div aria-hidden="true" className="h-full w-px shrink-0" style={{ background: T.line }} />
      <main className="flex h-full min-w-0 flex-1" aria-label={SETTINGS_PAGE_TITLES[page]}>
        {page === "general" && <GeneralPage />}
        {page === "quotas" && <QuotaPage />}
        {page === "sync" && <SyncPage />}
        {page === "directories" && <DirectoriesPage />}
        {page === "about" && <AboutPage />}
      </main>
    </div>
  );
}

function Sidebar() {
  const { page, setPage, status, syncState, appName } = useSettings();
  const dot = syncSidebarDot(syncState, status?.configured ?? false);
  return (
    <nav
      aria-label="设置"
      className="flex h-full shrink-0 flex-col"
      style={{
        width: T.sidebarWidth - 1,
        background: T.sidebarBackground,
        padding: "16px 10px 12px",
        gap: 1,
      }}
    >
      {SETTINGS_PAGES.map((item) => {
        const selected = item === page;
        return (
          <button
            key={item}
            type="button"
            aria-current={selected ? "page" : undefined}
            onClick={() => setPage(item)}
            className="settings-focus settings-sidebar-item flex shrink-0 items-center"
            style={{
              height: 30,
              gap: 9,
              padding: "0 9px",
              borderRadius: 7,
              color: T.text,
              background: selected ? T.selection : undefined,
            }}
          >
            <span className="inline-flex shrink-0 items-center justify-center" style={{ width: 16, height: 16 }}>
              {PAGE_ICONS[item]}
            </span>
            <span style={TYPE.label}>{SETTINGS_PAGE_TITLES[item]}</span>
            <span className="grow" style={{ minWidth: 4 }} />
            {/* One mark only: a red dot on 数据同步 after a failed sync, seen
                from every page (maintainer 2026-11-20: no count, no green
                dot, no 「高级」 chip in the sidebar). */}
            {item === "sync" && dot && (
              <span style={{ paddingRight: 4 }} aria-label="同步失败">
                <StatusDot kind={dot} />
              </span>
            )}
          </button>
        );
      })}
      <span className="grow" />
      <div className="flex items-center" style={{ padding: "0 8px", color: T.secondaryText }}>
        <AppIcon size={18} />
        <span className="min-w-0 truncate" style={{ ...TYPE.caption, paddingLeft: 8 }}>
          {appName}
        </span>
        <span className="grow" style={{ minWidth: 4 }} />
        <span className="shrink-0" style={{ fontSize: 11, fontFamily: T.mono }}>
          {status?.version ?? ""}
        </span>
      </div>
    </nav>
  );
}
