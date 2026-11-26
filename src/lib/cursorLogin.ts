// The Cursor sign-in row in Settings (`services::cursor_login`). Words are
// the plan's (《Cursor-加密凭据-方案》 §3, revised after the 2026-11-25
// real-account check) and the same as macOS, character for character.

import type { CursorLoginError, CursorLoginStatus } from "./types";

/** The grey line under the row, in every state. */
export const CURSOR_LOGIN_NOTE = "以 Cursor 命令行的身份登录，只用来读用量";

/** Some accounts are asked to confirm by email after the browser step. */
export const CURSOR_LOGIN_WAITING = "请在浏览器里确认；如果 Cursor 让你去邮箱确认，点完邮件里的链接再回来";

/** Shown before signing out. The revocation sentence is the CLI's own
 *  `logout cursor` notice (vibe-cafe/vibe-usage#210 `fc45047`): cursor.com
 *  lists this sign-in as 「Desktop App」, one more per renewal, and a
 *  revocation takes up to 10 minutes. */
export const CURSOR_LOGOUT_CONFIRM =
  "退出后不再读取 Cursor 用量。Cursor 官网上的这条登录可在 cursor.com 设置的 Sessions 里撤销：它显示为「Desktop App」（每续期一次多一条，按时间认，别撤正在用的 Cursor 编辑器那条），最多 10 分钟生效。";

export interface CursorLoginRow {
  /** Left of the button; null leaves only the button. */
  text: string | null;
  /** null: no button (the CLI does not offer the sign-in). */
  action: { label: string; kind: "start" | "cancel" | "logout" } | null;
  error: string | null;
  /** The grey line is shown with the sign-in button. */
  showNote: boolean;
}

export function cursorLoginErrorText(error: CursorLoginError): string {
  switch (error) {
    case "timeout":
      return "5 分钟内没有确认，已取消登录";
    case "network":
      return "连不上 Cursor，请检查网络和代理后重试";
    case "proxy":
      return "无法按你的代理设置连接 Cursor，未发送任何请求";
    case "badUrl":
      return "登录地址不对，已取消";
    case "saveFailed":
      return "登录成功，但没能保存，请重试";
    case "failed":
    default:
      return "Cursor 没有完成登录，请重试";
  }
}

/**
 * What the row shows. Signing in (and the grey line) needs a CLI that offers
 * `login cursor`; a sign-in already kept can always be signed out of.
 */
export function cursorLoginRow(status: CursorLoginStatus): CursorLoginRow {
  const offered = status.supported === true;
  const error = status.error ? cursorLoginErrorText(status.error) : null;
  switch (status.state) {
    case "waiting":
      return { text: CURSOR_LOGIN_WAITING, action: { label: "取消", kind: "cancel" }, error: null, showNote: true };
    case "signedIn":
      return {
        text: status.email ?? "Cursor 账号",
        action: { label: "退出", kind: "logout" },
        error,
        showNote: offered,
      };
    case "expired":
      return {
        text: "登录已失效",
        action: offered ? { label: "重新登录", kind: "start" } : { label: "退出", kind: "logout" },
        error,
        showNote: offered,
      };
    case "signedOut":
    default:
      return {
        text: null,
        action: offered ? { label: "登录", kind: "start" } : null,
        error: offered ? error : null,
        showNote: offered,
      };
  }
}
