import { describe, expect, it } from "vitest";
import {
  CURSOR_LOGIN_WAITING,
  CURSOR_LOGOUT_CONFIRM,
  CURSOR_LOGOUT_TITLE,
  cursorLoginErrorText,
  cursorLoginRow,
} from "../src/lib/cursorLogin";
import {
  quotaEmptyStateText,
  quotaProductStatusText,
  quotaSignInText,
} from "../src/lib/quotaProducts";
import type { CursorLoginStatus, ProviderRateLimit, QuotaProduct } from "../src/lib/types";

const status = (extra: Partial<CursorLoginStatus>): CursorLoginStatus => ({
  state: "signedOut",
  supported: true,
  ...extra,
});

describe("Cursor sign-in row (plan §3, same words as macOS)", () => {
  it("shows the four states with their one button", () => {
    expect(cursorLoginRow(status({}))).toEqual({
      text: null,
      action: { label: "登录", kind: "start" },
      secondary: null,
      error: null,
    });
    expect(cursorLoginRow(status({ state: "waiting" }))).toMatchObject({
      text: CURSOR_LOGIN_WAITING,
      action: { label: "取消", kind: "cancel" },
    });
    expect(cursorLoginRow(status({ state: "signedIn", email: "someone@example.com" }))).toMatchObject({
      text: "someone@example.com",
      action: { label: "退出", kind: "logout" },
    });
    expect(cursorLoginRow(status({ state: "signedIn" })).text).toBe("Cursor 账号");
    expect(cursorLoginRow(status({ state: "expired", email: "someone@example.com" }))).toMatchObject({
      text: "登录已失效",
      action: { label: "重新登录", kind: "start" },
      // An ended sign-in can also just be removed (back to Cursor's local login).
      secondary: { label: "退出", kind: "logout" },
    });
  });

  it("asks for email confirmation while waiting", () => {
    expect(CURSOR_LOGIN_WAITING).toContain("请在浏览器里确认");
    expect(CURSOR_LOGIN_WAITING).toContain("如果 Cursor 让你去邮箱确认，点完邮件里的链接再回来");
  });

  it("offers no sign-in without a CLI that has it, but always lets a kept one go", () => {
    for (const supported of [false, null]) {
      expect(cursorLoginRow(status({ supported }))).toMatchObject({ text: null, action: null });
      expect(cursorLoginRow(status({ supported, state: "signedIn", email: "a@b.c" })).action?.kind).toBe("logout");
      expect(cursorLoginRow(status({ supported, state: "expired" })).action?.kind).toBe("logout");
    }
  });

  it("names why a sign-in did not complete", () => {
    expect(cursorLoginRow(status({ error: "timeout" })).error).toBe("5 分钟内没有确认，已取消登录");
    expect(cursorLoginErrorText("network")).toBe("连不上 Cursor，请检查网络和代理后重试");
    expect(cursorLoginErrorText("proxy")).toBe("无法按你的代理设置连接 Cursor，未发送任何请求");
    expect(cursorLoginErrorText("failed")).toBe("Cursor 没有完成登录，请重试");
    expect(cursorLoginErrorText("badUrl")).toBe("登录地址不对，已取消");
    expect(cursorLoginErrorText("saveFailed")).toBe("登录成功，但没能保存，请重试");
    // Same words as macOS (its extra cases, 2026-11-26).
    expect(cursorLoginErrorText("proxyChanged")).toBe("代理设置变了，请重新登录");
    expect(cursorLoginErrorText("runtime")).toBe("没有可用的 Node.js 或 Bun");
    // Waiting again hides it (the backend drops it too).
    expect(cursorLoginRow(status({ state: "waiting", error: "timeout" })).error).toBeNull();
  });

  it("asks before signing out in one sentence (the rest is in PRIVACY.md)", () => {
    expect(CURSOR_LOGOUT_TITLE).toBe("退出 Cursor 登录？");
    expect(CURSOR_LOGOUT_CONFIRM).toBe("只删除 Vibe Usage 里的这份登录，Cursor 编辑器不受影响。");
  });
});

describe("Cursor card sign-in prompt", () => {
  const cursor: QuotaProduct = { provider: "cursor", displayName: "Cursor", availability: "ready", isDetected: true };
  const snapshot = (extra: Partial<ProviderRateLimit>): ProviderRateLimit => ({
    provider: "cursor",
    status: { kind: "noData" },
    fetchedAt: 1,
    ...extra,
  });

  it("repeats only what the CLI asked for", () => {
    expect(quotaSignInText("required")).toBe("需要登录 Cursor");
    expect(quotaSignInText("expired")).toBe("Cursor 登录已失效");
    const required = snapshot({ signIn: "required", liveFailure: { code: "credentialsEncrypted" } });
    const expired = snapshot({ signIn: "expired", liveFailure: { code: "credentialsRejected" } });
    expect(quotaEmptyStateText(required, true)).toBe("需要登录 Cursor");
    expect(quotaEmptyStateText(expired, true)).toBe("Cursor 登录已失效");
    expect(quotaProductStatusText(cursor, ["cursor"], required)).toBe("需登录");
    expect(quotaProductStatusText(cursor, ["cursor"], expired)).toBe("已失效");
    // An older CLI without `signIn`: unchanged.
    const old = snapshot({ liveFailure: { code: "credentialsEncrypted" } });
    expect(quotaEmptyStateText(old, true)).toBe("登录信息已加密，暂时读不了");
    expect(quotaProductStatusText(cursor, ["cursor"], old)).toBe("不支持");
    // Still reading: no prompt yet.
    expect(quotaEmptyStateText(required, true, true)).toBe("正在读取订阅配额…");
  });
});
