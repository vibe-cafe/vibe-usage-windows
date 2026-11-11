// Footer 「发现更新」 button states. The click downloads the installer (tens of
// MB) before anything else is visible, then the app quits and a passive
// installer shows only a progress bar, so the button itself has to show that
// work is under way and why it stopped.

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "downloading" }
  | { kind: "launching" }
  | { kind: "failed"; reason: string };

export function updateButtonLabel(phase: UpdatePhase): string {
  switch (phase.kind) {
    case "idle":
      return "发现更新";
    case "downloading":
      return "正在下载更新…";
    case "launching":
      return "正在启动安装…";
    case "failed":
      return `更新失败：${phase.reason}，点此重试`;
  }
}

/** Only idle and failed accept a click; the backend refuses a second install too. */
export function updateButtonEnabled(phase: UpdatePhase): boolean {
  return phase.kind === "idle" || phase.kind === "failed";
}

/** Tauri rejects a command with the Rust `Err(String)` as-is. */
export function updateFailureReason(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error ?? "");
  return text.trim() || "未知错误";
}
