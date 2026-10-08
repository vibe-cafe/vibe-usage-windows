import { afterEach, expect, test, vi } from "vitest";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { cliPath } from "./cliUnderTest.mjs";

const originalSurface = process.env.VIBE_USAGE_SURFACE;
const originalSurfaceVersion = process.env.VIBE_USAGE_SURFACE_VERSION;

afterEach(() => {
  if (originalSurface === undefined) delete process.env.VIBE_USAGE_SURFACE;
  else process.env.VIBE_USAGE_SURFACE = originalSurface;
  if (originalSurfaceVersion === undefined) delete process.env.VIBE_USAGE_SURFACE_VERSION;
  else process.env.VIBE_USAGE_SURFACE_VERSION = originalSurfaceVersion;
  vi.resetModules();
});

test("published CLI reports its real version and the Windows App identity", async () => {
  const appPackage = JSON.parse(readFileSync("package.json", "utf-8"));
  const cliPackage = JSON.parse(
    readFileSync(cliPath("package.json"), "utf-8"),
  );
  process.env.VIBE_USAGE_SURFACE = "windows-app";
  process.env.VIBE_USAGE_SURFACE_VERSION = appPackage.version;
  vi.resetModules();

  const { createSyncClient } = await import(
    pathToFileURL(cliPath("src/client-meta.js")).href
  );
  const client = createSyncClient({ hostname: "windows-pc" });

  expect(client.collectorVersion).toBe(cliPackage.version);
  expect(client.surface).toBe("windows-app");
  expect(client.surfaceVersion).toBe(appPackage.version);
});

test("Tauri sync injects the Windows App surface and package version", () => {
  const source = readFileSync("src-tauri/src/services/sync_engine.rs", "utf-8");
  expect(source).toContain('cmd.env("VIBE_USAGE_SURFACE", "windows-app")');
  expect(source).toContain('"VIBE_USAGE_SURFACE_VERSION"');
  expect(source).toContain("app.package_info().version.to_string()");
});

test("the app resolves the CLI from npm's latest at run time", () => {
  const packageJson = JSON.parse(readFileSync("package.json", "utf-8"));
  const tauriConf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf-8"));
  const workflow = readFileSync(".github/workflows/release.yml", "utf-8");
  const localRelease = readFileSync("scripts/release-windows.ps1", "utf-8");

  // A version pin is exactly what run-time resolution replaced: it is the thing
  // that freezes users out of every CLI fix without anyone noticing.
  expect(packageJson.vibeUsageCliChannel).toBe("latest");
  expect(packageJson.vibeUsageCliVersion).toBeUndefined();
  expect(tauriConf.bundle.resources).toEqual({
    "resources/cli-bootstrap.mjs": "cli-bootstrap.mjs",
  });
  // The launcher is the app's only path to the CLI, so a release has to prove it
  // can resolve and execute latest.
  expect(workflow).toContain("cli-bootstrap.mjs --version");
  for (const source of [workflow, localRelease]) {
    expect(source).not.toContain("scripts/vendor-cli.mjs");
  }
});
