import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { providerGlyphSize } from "../src/components/ProviderIcon";
import { RateLimitProvider } from "../src/lib/types";

const PROVIDERS: RateLimitProvider[] = [
  "codex",
  "claudeCode",
  "kimi-code",
  "zcode",
  "grok",
  "cursor",
  "opencode-go",
  "antigravity",
];

describe("monochrome provider marks", () => {
  it("draws every catalog product, Antigravity included, inside its slot", () => {
    for (const provider of PROVIDERS) {
      const drawn = providerGlyphSize(provider, 20);
      expect(drawn).toBeGreaterThan(0);
      expect(drawn).toBeLessThanOrEqual(20);
    }
  });

  it("uses the macOS sizing: size × 18 / 22, then the per-mark fill ratio", () => {
    // Codex / Claude / Cursor are 15 pt in an 18 pt glyph box on macOS.
    expect(providerGlyphSize("codex", 22)).toBeCloseTo(18 * 0.833);
    expect(providerGlyphSize("kimi-code", 22)).toBeCloseTo(18 * 0.994);
  });

  it("no longer ships the coloured vendor PNGs", () => {
    const source = readFileSync("src/components/ProviderIcon.tsx", "utf-8");
    expect(source).not.toMatch(/\.png/);
    // The Lobe Icons MIT notice travels with the path data.
    expect(source).toContain("LobeHub");
    expect(readFileSync("THIRD_PARTY_NOTICES.md", "utf-8")).toContain("Copyright (c) 2023 LobeHub");
  });
});
