import { expect, test } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { cliPath } from "./cliUnderTest.mjs";

// Asserts the quota contract of the CLI the app actually runs (resolved from
// npm's `latest` dist-tag) — the Rust side parses exactly this shape.
test("published CLI returns the quota contract without inherited account credentials", () => {
  const root = mkdtempSync(join(tmpdir(), "vibe-windows-quota-"));
  try {
    // A temporary home and a PATH with only Node and the system directories:
    // since vibe-cafe/vibe-usage#189 the CLI reads Grok by starting the official
    // `grok`, and must not find the developer's or release machine's own copy,
    // which would ask xAI with that machine's sign-in and change the status.
    const systemPath = process.platform === "win32"
      ? [join(process.env.SystemRoot || "C:\\Windows", "System32")]
      : ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];
    const env: NodeJS.ProcessEnv = {
      PATH: [dirname(process.execPath), ...systemPath].join(delimiter),
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
      KIMI_CODE_HOME: join(root, "kimi-code"),
      KIMI_SHARE_DIR: join(root, "kimi"),
      GROK_HOME: join(root, "grok"),
      // Points at nothing, so a developer's own signed-in `agy` never runs.
      ANTIGRAVITY_CLI_PATH: join(root, "agy-absent", "agy.exe"),
    };
    const invoke = (...args: string[]) => {
      const result = spawnSync(process.execPath, [
        cliPath("bin/vibe-usage.js"), ...args,
      ], { env, cwd: root, encoding: "utf8", timeout: 10_000 });
      expect(result.error).toBeUndefined();
      expect(result.status).toBe(0);
      return JSON.parse(result.stdout);
    };
    expect(invoke("config", "roots")).toEqual({});
    const discovery = invoke("quota", "discover", "--json");
    expect(discovery.schemaVersion).toBe(1);
    for (const id of ["kimi-code", "zcode", "grok", "antigravity"]) {
      expect(discovery.products.find((p: { id: string }) => p.id === id)?.fetchable).toBe(true);
    }
    // Cursor is deliberately not asserted: the app shows it as 待接入 and never
    // routes it through the CLI, while the CLI made it fetchable in
    // vibe-cafe/vibe-usage#128 — its value is not part of this app's contract.
    const quota = invoke("quota", "fetch", "--product", "kimi-code",
      "--product", "zcode", "--product", "grok", "--product", "antigravity", "--json");
    expect(quota.schemaVersion).toBe(1);
    expect(quota.products.map((p: { id: string; status: string; meters: unknown[] }) =>
      ({ id: p.id, status: p.status, meters: p.meters }))).toEqual([
      { id: "kimi-code", status: "missing_credentials", meters: [] },
      { id: "zcode", status: "missing_credentials", meters: [] },
      { id: "grok", status: "no_data", meters: [] },
      // No `agy` found: the card asks to install it and sign in.
      { id: "antigravity", status: "missing_credentials", meters: [] },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}, 30_000);
