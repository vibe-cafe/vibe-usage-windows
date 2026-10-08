import { afterEach, beforeAll, expect, test } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { extraRootsLoadPatch, formatInvokeError } from "../src/lib/extraRoots";
import { ExtraRoots } from "../src/lib/types";
import { cliPath } from "./cliUnderTest.mjs";

// The CLI is not bundled any more — the app resolves `@vibe-cafe/vibe-usage@latest`
// at run time — so both the entry point and the validator below come from the
// published package this run resolved.
const cliEntry = cliPath("bin/vibe-usage.js");

type ValidateExtraRoot = (
  source: string,
  value: string,
) => { ok: boolean; path: unknown; reason?: string };
let validateExtraRoot: ValidateExtraRoot;

// A static import is impossible here: the path only exists after the launcher
// resolves `latest`, so the specifier is runtime-selected.
beforeAll(async () => {
  const mod = (await import(pathToFileURL(cliPath("src/extra-roots.js")).href)) as {
    validateExtraRoot: ValidateExtraRoot;
  };
  validateExtraRoot = mod.validateExtraRoot;
});

const tempDirs: string[] = [];

function makeTempDir() {
  const dir = mkdtempSync(join(tmpdir(), "vibe-extra-roots-"));
  tempDirs.push(dir);
  return dir;
}

function runCli(configDir: string, args: string[]) {
  return spawnSync(process.execPath, [cliEntry, ...args], {
    env: { ...process.env, VIBE_USAGE_CONFIG_DIR: configDir, NO_COLOR: "1" },
    encoding: "utf8",
  });
}

afterEach(() => {
  while (tempDirs.length > 0) {
    rmSync(tempDirs.pop()!, { recursive: true, force: true });
  }
});

test("formatInvokeError reads Tauri object payloads instead of [object Object]", () => {
  const err = { message: "需要包含 sessions/" };
  expect(String(err)).toBe("[object Object]");
  expect(formatInvokeError(err)).toBe("需要包含 sessions/");
  expect(formatInvokeError(new Error("CLI 配置操作失败"))).toBe("CLI 配置操作失败");
  expect(formatInvokeError("  目录无效  ")).toBe("目录无效");
});

test("getExtraRoots failure is visible and does not replace lists with empty", () => {
  const previous: ExtraRoots = { grok: ["C:\\\\old-root"] };
  const result: PromiseSettledResult<ExtraRoots> = {
    status: "rejected",
    reason: { message: "无法读取隔离运行时目录: unexpected eof" },
  };
  const patch = extraRootsLoadPatch(result);
  expect(patch.extraRoots).toBeUndefined();
  expect(patch.extraRootsError).toContain("无法读取隔离运行时目录");
  expect(patch.extraRoots ?? previous).toEqual(previous);
});

test("successful extra-roots load clears the error and replaces the list", () => {
  const result: PromiseSettledResult<ExtraRoots> = {
    status: "fulfilled",
    value: { grok: ["/tmp/grok"] },
  };
  expect(extraRootsLoadPatch(result)).toEqual({
    extraRoots: { grok: ["/tmp/grok"] },
    extraRootsError: null,
  });
});

test("invalid isolated runtime layout is rejected", () => {
  const empty = makeTempDir();
  expect(validateExtraRoot("grok", empty).ok).toBe(false);
  expect(validateExtraRoot("antigravity", empty).ok).toBe(false);
  expect(validateExtraRoot("antigravity", empty).reason).toContain(".gemini/antigravity");
  expect(validateExtraRoot("cursor", empty).ok).toBe(false);
});

test("CLI add-root / roots / remove-root round-trips a valid grok layout", () => {
  const configDir = makeTempDir();
  const grokRoot = makeTempDir();
  mkdirSync(join(grokRoot, "sessions"));

  const added = runCli(configDir, ["config", "add-root", "grok", grokRoot]);
  expect(added.status, added.stderr || added.stdout).toBe(0);

  const listed = runCli(configDir, ["config", "roots"]);
  expect(listed.status, listed.stderr || listed.stdout).toBe(0);
  const roots = JSON.parse(listed.stdout) as ExtraRoots;
  expect(roots.grok).toEqual([resolve(grokRoot)]);

  const removed = runCli(configDir, ["config", "remove-root", "grok", grokRoot]);
  expect(removed.status, removed.stderr || removed.stdout).toBe(0);

  const after = runCli(configDir, ["config", "roots"]);
  expect(after.status).toBe(0);
  expect(JSON.parse(after.stdout)).toEqual({});
});

test("CLI add-root surfaces invalid layout instead of recording an empty root", () => {
  const configDir = makeTempDir();
  const invalid = makeTempDir();
  const added = runCli(configDir, ["config", "add-root", "antigravity", invalid]);
  expect(added.status).not.toBe(0);
  expect(`${added.stderr}\n${added.stdout}`).toMatch(/antigravity|conversations|无效/);

  const listed = runCli(configDir, ["config", "roots"]);
  expect(listed.status).toBe(0);
  expect(JSON.parse(listed.stdout)).toEqual({});
});
