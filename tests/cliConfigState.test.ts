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
import { pathToFileURL } from "node:url";
import { cliPath } from "./cliUnderTest.mjs";

// The modules under test live in the published CLI the launcher resolved, so they
// are described structurally instead of importing their types by path.
type ConfigModule = { saveConfig: (config: { apiKey: string; apiUrl: string }) => void };
type StateModule = {
  saveState: (state: { buckets: Record<string, string>; sessions: Record<string, unknown> }) => void;
};

const originalConfigDir = process.env.VIBE_USAGE_CONFIG_DIR;
const originalStateDir = process.env.VIBE_USAGE_STATE_DIR;
const originalDev = process.env.VIBE_USAGE_DEV;
const tempDirs: string[] = [];

function makeTempDir() {
  const dir = mkdtempSync(join(tmpdir(), "vibe-usage-cli-"));
  tempDirs.push(dir);
  return dir;
}

async function importWithConfigDir<T>(path: string, dir: string): Promise<T> {
  // Mirrors what sync_engine.rs exports to the CLI: the state file follows
  // VIBE_USAGE_STATE_DIR, which the app points at the same directory.
  process.env.VIBE_USAGE_CONFIG_DIR = dir;
  process.env.VIBE_USAGE_STATE_DIR = dir;
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

  if (originalStateDir === undefined) {
    delete process.env.VIBE_USAGE_STATE_DIR;
  } else {
    process.env.VIBE_USAGE_STATE_DIR = originalStateDir;
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

  const config = await importWithConfigDir<ConfigModule>(
    pathToFileURL(cliPath("src/config.js")).href,
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

  const state = await importWithConfigDir<StateModule>(
    pathToFileURL(cliPath("src/state.js")).href,
    dir,
  );
  state.saveState({ buckets: { a: "b" }, sessions: {} });

  const parsed = JSON.parse(readFileSync(join(dir, "state.json"), "utf-8"));
  expect(parsed.buckets.a).toBe("b");
  expect(readdirSync(dir).some((name) => name.startsWith("state.json.directory-backup-"))).toBe(
    true,
  );
});
