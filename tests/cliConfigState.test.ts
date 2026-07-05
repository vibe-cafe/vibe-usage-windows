import { afterEach, expect, test, vi } from "vitest";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const originalConfigDir = process.env.VIBE_USAGE_CONFIG_DIR;
const originalDev = process.env.VIBE_USAGE_DEV;
const tempDirs: string[] = [];

function makeTempDir() {
  const dir = mkdtempSync(join(tmpdir(), "vibe-usage-cli-"));
  tempDirs.push(dir);
  return dir;
}

async function importWithConfigDir<T>(path: string, dir: string): Promise<T> {
  process.env.VIBE_USAGE_CONFIG_DIR = dir;
  delete process.env.VIBE_USAGE_DEV;
  vi.resetModules();
  return import(path) as Promise<T>;
}

afterEach(() => {
  if (originalConfigDir === undefined) {
    delete process.env.VIBE_USAGE_CONFIG_DIR;
  } else {
    process.env.VIBE_USAGE_CONFIG_DIR = originalConfigDir;
  }

  if (originalDev === undefined) {
    delete process.env.VIBE_USAGE_DEV;
  } else {
    process.env.VIBE_USAGE_DEV = originalDev;
  }

  vi.resetModules();
  while (tempDirs.length > 0) {
    rmSync(tempDirs.pop()!, { recursive: true, force: true });
  }
});

test("saveConfig repairs a directory occupying config.json", async () => {
  const dir = makeTempDir();
  mkdirSync(join(dir, "config.json"));

  const config = await importWithConfigDir<typeof import("../src-tauri/resources/cli/src/config.js")>(
    "../src-tauri/resources/cli/src/config.js",
    dir,
  );
  config.saveConfig({ apiKey: "vbu_test", apiUrl: "https://vibecafe.ai" });

  const parsed = JSON.parse(readFileSync(join(dir, "config.json"), "utf-8"));
  expect(parsed.apiKey).toBe("vbu_test");
  expect(readdirSync(dir).some((name) => name.startsWith("config.json.directory-backup-"))).toBe(
    true,
  );
});

test("saveState repairs a directory occupying state.json", async () => {
  const dir = makeTempDir();
  mkdirSync(join(dir, "state.json"));

  const state = await importWithConfigDir<typeof import("../src-tauri/resources/cli/src/state.js")>(
    "../src-tauri/resources/cli/src/state.js",
    dir,
  );
  state.saveState({ buckets: { a: "b" }, sessions: {} });

  const parsed = JSON.parse(readFileSync(join(dir, "state.json"), "utf-8"));
  expect(parsed.buckets.a).toBe("b");
  expect(readdirSync(dir).some((name) => name.startsWith("state.json.directory-backup-"))).toBe(
    true,
  );
});
