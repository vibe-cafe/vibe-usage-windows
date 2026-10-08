import { expect, test } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { cliPath } from "./cliUnderTest.mjs";

// Asserts the quota contract of the CLI the app actually runs (resolved from
// npm's `latest` dist-tag) — the Rust side parses exactly this shape.
test("published CLI returns the quota contract without inherited account credentials", () => {
  const root = mkdtempSync(join(tmpdir(), "vibe-windows-quota-"));
  try {
    const env: NodeJS.ProcessEnv = {
      PATH: process.env.PATH,
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
    for (const id of ["kimi-code", "zcode", "grok"]) {
      expect(discovery.products.find((p: { id: string }) => p.id === id)?.fetchable).toBe(true);
    }
    expect(discovery.products.find((p: { id: string }) => p.id === "cursor")?.fetchable).toBe(false);
    const quota = invoke("quota", "fetch", "--product", "kimi-code",
      "--product", "zcode", "--product", "grok", "--json");
    expect(quota.schemaVersion).toBe(1);
    expect(quota.products.map((p: { id: string; status: string; meters: unknown[] }) =>
      ({ id: p.id, status: p.status, meters: p.meters }))).toEqual([
      { id: "kimi-code", status: "missing_credentials", meters: [] },
      { id: "zcode", status: "missing_credentials", meters: [] },
      { id: "grok", status: "no_data", meters: [] },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}, 30_000);
