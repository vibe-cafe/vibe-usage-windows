// Header — port of PopoverView.headerBar.

import { useEffect, useState } from "react";
import { AlertCircle, ArrowUpCircle, ArrowUpRight } from "lucide-react";
import { api } from "../lib/api";
import { useAppState } from "../state/AppStateContext";
import {
  type UpdatePhase,
  createUpdateRunner,
  phaseForNewOffer,
  updateButtonEnabled,
  updateButtonLabel,
} from "../lib/updateButton";

export function HeaderBar() {
  const { status } = useAppState();
  const base = status?.apiUrl.replace(/\/+$/, "");

  return (
    <div className="flex items-center gap-1.5">
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="shrink-0 text-[15px] font-bold text-white">Vibe Usage</span>
        {status?.isDev && (
          <span
            className="shrink-0 rounded-[3px] px-1 py-px font-mono text-[10px] font-bold text-orange-400"
            style={{ background: "rgba(251,146,60,0.15)" }}
          >
            DEBUG
          </span>
        )}
        <UpdateBadge />
      </div>

      <div className="grow" />

      <HeaderLinkButton title="详情" url={base && `${base}/usage`} />
      <HeaderLinkButton title="排行榜" url={base && `${base}/rank`} />

      <button className="header-btn" onClick={() => void api.openSettingsWindow()} style={btnStyle}>
        设置
      </button>
    </div>
  );
}

/** macOS `Color(red: 0.4, green: 0.7, blue: 1.0)` — the panel's link blue. */
const UPDATE_BLUE = "#66B3FF";

/**
 * 「发现更新」 beside the app title, where it stays visible without competing
 * with the footer's actions (macOS `PopoverView.headerBar`). One click
 * downloads and starts the installer — Windows does not ask a second time
 * (docs/PARITY.md, 平台差异 5) — so the badge itself shows the download and
 * why it stopped.
 */
function UpdateBadge() {
  const { updateInfo } = useAppState();
  const [phase, setPhase] = useState<UpdatePhase>({ kind: "idle" });
  const [startUpdate] = useState(() => createUpdateRunner(api.installUpdate, setPhase));
  // The badge disappears with the offer (updateInfo null after
  // `update-cleared`); a failure message never carries over to the next one.
  const offeredVersion = updateInfo?.version ?? null;
  useEffect(() => {
    setPhase(phaseForNewOffer);
  }, [offeredVersion]);

  if (!updateInfo) return null;
  const label = updateButtonLabel(phase);
  const busy = phase.kind === "downloading" || phase.kind === "launching";
  return (
    <button
      type="button"
      className="flex min-w-0 max-w-[220px] items-center gap-[3px] rounded-[3px] px-[5px] py-px text-[10px] font-medium disabled:opacity-60"
      style={{ color: UPDATE_BLUE, background: "rgba(102,179,255,0.15)" }}
      disabled={!updateButtonEnabled(phase)}
      title={phase.kind === "idle" ? `发现新版本 ${updateInfo.version}，点击更新` : label}
      onClick={() => void startUpdate()}
    >
      {busy ? (
        <div className="spinner h-2.5 w-2.5 shrink-0" />
      ) : phase.kind === "failed" ? (
        <AlertCircle size={10} className="shrink-0" />
      ) : (
        <ArrowUpCircle size={10} className="shrink-0" />
      )}
      <span className="truncate">{label}</span>
    </button>
  );
}

const btnStyle: React.CSSProperties = {
  fontSize: 11,
  color: "#808080",
  padding: "4px 8px",
  background: "rgba(255,255,255,0.12)",
  borderRadius: 4,
  border: "0.5px solid rgba(255,255,255,0.18)",
  display: "inline-flex",
  alignItems: "center",
  gap: 3,
  lineHeight: 1.2,
};

function HeaderLinkButton({ title, url }: { title: string; url?: string }) {
  return (
    <button
      className="shrink-0"
      style={btnStyle}
      disabled={!url}
      onClick={url ? () => void api.openExternal(url) : undefined}
    >
      {title}
      <ArrowUpRight size={9} strokeWidth={2.5} />
    </button>
  );
}
