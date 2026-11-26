// The Cursor sign-in row in Settings (`services::cursor_login`). Words are
// the plan's (《Cursor-加密凭据-方案》 §3, revised after the 2026-11-25
// real-account check) and the same as macOS, character for character.

import type { CursorLoginError, CursorLoginStatus } from "./types";

/** The grey line under the row, in every state. */
export const CURSOR_LOGIN_NOTE = "以 Cursor 命令行的身份登录，只用来读用量";

/** Some accounts are asked to confirm by email after the browser step. */
export const CURSOR_LOGIN_WAITING = "请在浏览器里确认；如果 Cursor 让你去邮箱确认，点完邮件里的链接再回来";

/** Asked before signing out: one sentence (maintainer, 2026-11-26: the
 *  earlier paragraph was too long). How to revoke the session on cursor.com,
 *  and that Cursor's own local login is still read, are in PRIVACY.md. */
export const CURSOR_LOGOUT_TITLE = "退出 Cursor 登录？";
export const CURSOR_LOGOUT_CONFIRM = "只删除 Vibe Usage 里的这份登录，Cursor 编辑器不受影响。";

export type CursorLoginAction = { label: string; kind: "start" | "cancel" | "logout" };

export interface CursorLoginRow {
  /** Left of the button; null leaves only the button. */
  text: string | null;
  /** null: no button (the CLI does not offer the sign-in). */
  action: CursorLoginAction | null;
  /** A quieter second button: an ended sign-in can also just be removed, so
   *  the CLI goes back to Cursor's own local login. */
  secondary: CursorLoginAction | null;
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
    case "proxyChanged":
      return "代理设置变了，请重新登录";
    case "runtime":
      return "没有可用的 Node.js 或 Bun";
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
      return {
        text: CURSOR_LOGIN_WAITING,
        action: { label: "取消", kind: "cancel" },
        secondary: null,
        error: null,
        showNote: true,
      };
    case "signedIn":
      return {
        text: status.email ?? "Cursor 账号",
        action: { label: "退出", kind: "logout" },
        secondary: null,
        error,
        showNote: offered,
      };
    case "expired":
      return offered
        ? {
            text: "登录已失效",
            action: { label: "重新登录", kind: "start" },
            secondary: { label: "退出", kind: "logout" },
            error,
            showNote: true,
          }
        : { text: "登录已失效", action: { label: "退出", kind: "logout" }, secondary: null, error, showNote: false };
    case "signedOut":
    default:
      return {
        text: null,
        action: offered ? { label: "登录", kind: "start" } : null,
        secondary: null,
        error: offered ? error : null,
        showNote: offered,
      };
  }
}
