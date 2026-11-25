/**
 * Settings → 隐私保护 → 「没检测到代理时」, one of three (the maintainer's
 * wording, 2026-11-25; same as macOS `EgressCheck.Mode`). The backend maps it
 * to the egress check (`egress_check.rs`) and to the CLI's two variables.
 */
export type EgressMode = "auto" | "pause" | "allow";

export const EGRESS_MODES: { value: EgressMode; label: string; caption: string }[] = [
  {
    value: "auto",
    label: "自动判断（推荐）",
    caption: "网络在中国大陆等地区时，暂停读取 Codex、Claude、Grok、Antigravity，避免真实 IP 和账号一起暴露。",
  },
  {
    value: "pause",
    label: "暂停读取",
    caption: "暂停读取 Codex、Claude、Grok、Antigravity，检测到代理后自动继续。",
  },
  {
    value: "allow",
    label: "照常读取",
    caption: "照常读取 Codex、Claude、Grok、Antigravity，厂商可能看到你的真实 IP。",
  },
];

/** The option for a stored value; anything unknown reads as the default. */
export function egressModeOption(value: string | null | undefined) {
  return EGRESS_MODES.find((mode) => mode.value === value) ?? EGRESS_MODES[0];
}
