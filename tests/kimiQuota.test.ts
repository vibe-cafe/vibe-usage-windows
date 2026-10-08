import { beforeAll, expect, test } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { cliPath } from "./cliUnderTest.mjs";

type KimiQuotaModule = {
  kimiCredentialPath: (...args: never[]) => string;
  kimiCredentialPaths: (...args: never[]) => string[];
  parseKimiUsage: (usage: unknown) => unknown;
};
let kimiCredentialPath: KimiQuotaModule["kimiCredentialPath"];
let kimiCredentialPaths: KimiQuotaModule["kimiCredentialPaths"];
let parseKimiUsage: KimiQuotaModule["parseKimiUsage"];

// A static import cannot be used: the provider lives in the published CLI the
// launcher resolved from npm's `latest` dist-tag, so its path is runtime-selected.
beforeAll(async () => {
  const mod = (await import(
    pathToFileURL(cliPath("src/quotas/providers/kimi-code.js")).href
  )) as KimiQuotaModule;
  ({ kimiCredentialPath, kimiCredentialPaths, parseKimiUsage } = mod);
});

test("published CLI reads Kimi Code 2.x quota fields", () => {
  expect(parseKimiUsage({
    usages: {
      limit_5h: { used_ratio: 0.3, reset_time: "2026-09-11T18:00:00Z" },
      limit_7d: { used_ratio: 0.2, reset_time: "2026-09-17T00:00:00Z" },
    },
  })).toEqual([
    {
      id: "limit-5h", label: "5h", utilization: 30,
      resetsAt: "2026-09-11T18:00:00.000Z", windowSeconds: 18_000,
    },
    {
      id: "limit-7d", label: "7d", utilization: 20,
      resetsAt: "2026-09-17T00:00:00.000Z", windowSeconds: 604_800,
    },
  ]);
});

test("bundled CLI prefers the Kimi Code 2.x credential home", () => {
  const root = mkdtempSync(join(tmpdir(), "vibe-windows-kimi-home-"));
  const currentPath = join(root, ".kimi-code", "credentials", "kimi-code.json");
  const legacyPath = join(root, ".kimi", "credentials", "kimi-code.json");
  try {
    mkdirSync(join(root, ".kimi-code", "credentials"), { recursive: true });
    mkdirSync(join(root, ".kimi", "credentials"), { recursive: true });
    writeFileSync(currentPath, "{}");
    writeFileSync(legacyPath, "{}");

    expect(kimiCredentialPaths({}, root)).toEqual([currentPath, legacyPath]);
    expect(kimiCredentialPath({}, root)).toBe(currentPath);

    rmSync(join(root, ".kimi-code"), { recursive: true, force: true });
    expect(kimiCredentialPath({}, root)).toBe(legacyPath);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
