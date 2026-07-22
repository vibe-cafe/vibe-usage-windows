import { afterEach, expect, test, vi } from "vitest";
import { readFileSync } from "node:fs";

const originalSurface = process.env.VIBE_USAGE_SURFACE;
const originalSurfaceVersion = process.env.VIBE_USAGE_SURFACE_VERSION;

afterEach(() => {
  if (originalSurface === undefined) delete process.env.VIBE_USAGE_SURFACE;
  else process.env.VIBE_USAGE_SURFACE = originalSurface;
  if (originalSurfaceVersion === undefined) delete process.env.VIBE_USAGE_SURFACE_VERSION;
  else process.env.VIBE_USAGE_SURFACE_VERSION = originalSurfaceVersion;
  vi.resetModules();
});

test("vendored CLI reports its real version and the Windows App identity", async () => {
  process.env.VIBE_USAGE_SURFACE = "windows-app";
  process.env.VIBE_USAGE_SURFACE_VERSION = "0.5.10";
  vi.resetModules();

  const { createSyncClient } = await import(
    "../src-tauri/resources/cli/src/client-meta.js"
  );
  const client = createSyncClient({ hostname: "windows-pc" });

  expect(client.collectorVersion).toBe("0.10.2");
  expect(client.surface).toBe("windows-app");
  expect(client.surfaceVersion).toBe("0.5.10");
});

test("Tauri sync injects the Windows App surface and package version", () => {
  const source = readFileSync("src-tauri/src/services/sync_engine.rs", "utf-8");
  expect(source).toContain('cmd.env("VIBE_USAGE_SURFACE", "windows-app")');
  expect(source).toContain('"VIBE_USAGE_SURFACE_VERSION"');
  expect(source).toContain("app.package_info().version.to_string()");
});

test("release vendoring resolves npm latest without an implicit local fallback", () => {
  const packageJson = JSON.parse(readFileSync("package.json", "utf-8"));
  const vendorScript = readFileSync("scripts/vendor-cli.mjs", "utf-8");

  expect(packageJson.vibeUsageCliChannel).toBe("latest");
  expect(vendorScript).toContain("@vibe-cafe/vibe-usage@${CLI_CHANNEL}");
  expect(vendorScript).not.toContain("falling back to ../vibe-usage");
});
