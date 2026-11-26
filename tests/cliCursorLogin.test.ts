import { describe, expect, test } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { cliPath } from "./cliUnderTest.mjs";

// The stdin contract `services::cursor_login` relies on, against the CLI the
// app actually runs (npm `latest`). CLI 0.15.10 brings `login cursor`
// (vibe-cafe/vibe-usage#210); with an older `latest` these tests are skipped
// and turn on by themselves once it is published. No account, no network:
// only an ended sign-in is handed over, for which the CLI sends nothing.

function isolatedEnv(root: string): NodeJS.ProcessEnv {
  return {
    PATH: root,
    HOME: root,
    USERPROFILE: root,
    SystemRoot: process.env.SystemRoot,
    WINDIR: process.env.WINDIR,
    TEMP: root,
    TMP: root,
    TMPDIR: root,
    VIBE_USAGE_CONFIG_DIR: join(root, "config"),
    VIBE_USAGE_STATE_DIR: join(root, "state"),
    VIBE_USAGE_QUOTA_CACHE_DIR: join(root, "quota-cache"),
    VIBE_USAGE_SURFACE: "windows-app",
    // Never a developer's own Cursor login.
    CURSOR_STATE_DB_PATH: join(root, "cursor-absent", "state.vscdb"),
    // A request would fail at once instead of leaving the machine.
    HTTPS_PROXY: "http://127.0.0.1:9",
    HTTP_PROXY: "http://127.0.0.1:9",
    NODE_USE_ENV_PROXY: "1",
  };
}

function run(root: string, args: string[], input?: string) {
  return spawnSync(process.execPath, [cliPath("bin/vibe-usage.js"), ...args], {
    env: isolatedEnv(root),
    cwd: root,
    encoding: "utf8",
    input,
    timeout: 15_000,
  });
}

const probe = mkdtempSync(join(tmpdir(), "vibe-windows-cursor-login-"));
const offersLogin = run(probe, ["help", "--all"]).stdout.includes("login cursor");
rmSync(probe, { recursive: true, force: true });

describe.skipIf(!offersLogin)("CLI Cursor sign-in contract (stdin)", () => {
  const ended = JSON.stringify({ version: 1, rejectedAt: "2026-11-26T00:00:00.000Z" });

  test("an ended sign-in on stdin: the card is asked to offer signing in again, nothing is sent", () => {
    const root = mkdtempSync(join(tmpdir(), "vibe-windows-cursor-login-"));
    try {
      const result = run(root, ["quota", "fetch", "--product", "cursor", "--json", "--cursor-login-stdin"], ended);
      expect(result.status).toBe(0);
      const cursor = JSON.parse(result.stdout).products[0];
      expect(cursor).toMatchObject({ id: "cursor", status: "unsupported", signIn: "expired" });
      expect(cursor.failureReason).toBe("credentialsRejected");
      // The CLI kept nothing of it on disk.
      expect(readdirSync(root).some((name) => name.includes("cursor-login"))).toBe(false);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  test("renewing an ended sign-in sends nothing and answers in one JSON line", () => {
    const root = mkdtempSync(join(tmpdir(), "vibe-windows-cursor-login-"));
    try {
      const result = run(root, ["login", "cursor", "--refresh", "--json", "--cursor-login-stdin"], ended);
      // No usable sign-in on stdin: a usage error, as one event line.
      const events = result.stdout.trim().split("\n").map((line) => JSON.parse(line));
      expect(events).toHaveLength(1);
      expect(events[0].event).toBe("error");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
