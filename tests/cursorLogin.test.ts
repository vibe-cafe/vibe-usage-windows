import { describe, expect, it } from "vitest";
import {
  CURSOR_LOGIN_NOTE,
  CURSOR_LOGIN_WAITING,
  CURSOR_LOGOUT_CONFIRM,
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
      error: null,
      showNote: true,
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
    });
  });

  it("asks for email confirmation while waiting", () => {
    expect(CURSOR_LOGIN_WAITING).toContain("请在浏览器里确认");
    expect(CURSOR_LOGIN_WAITING).toContain("如果 Cursor 让你去邮箱确认，点完邮件里的链接再回来");
    expect(CURSOR_LOGIN_NOTE).toBe("以 Cursor 命令行的身份登录，只用来读用量");
  });

  it("offers no sign-in without a CLI that has it, but always lets a kept one go", () => {
    for (const supported of [false, null]) {
      expect(cursorLoginRow(status({ supported }))).toMatchObject({ text: null, action: null, showNote: false });
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
    // Waiting again hides it (the backend drops it too).
    expect(cursorLoginRow(status({ state: "waiting", error: "timeout" })).error).toBeNull();
  });

  it("explains what signing out does and where to revoke", () => {
    // Only this sign-in goes: Cursor's own local login, when readable, is still read.
    expect(CURSOR_LOGOUT_CONFIRM).toMatch(/^退出只删除 Vibe Usage 保存的这份登录；Cursor 本机的登录还能读时，仍会照旧读取用量。/);
    expect(CURSOR_LOGOUT_CONFIRM).toContain("cursor.com 设置的 Sessions 里撤销");
    expect(CURSOR_LOGOUT_CONFIRM).toContain("「Desktop App」");
    expect(CURSOR_LOGOUT_CONFIRM).toContain("每续期一次多一条");
    expect(CURSOR_LOGOUT_CONFIRM).toContain("别撤正在用的 Cursor 编辑器那条");
    expect(CURSOR_LOGOUT_CONFIRM).toContain("最多 10 分钟生效");
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
