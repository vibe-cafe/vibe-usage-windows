import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { dropdownLeft } from "../src/components/FilterTags";
import { quotaCardStep } from "../src/components/RateLimitCard";

// vibe-cafe/vibe-usage-windows#51: the main window was a fixed 520×620, taller
// than the screen at e.g. 1366×768 + 125% (about 614 logical px).

const DEFAULT_ROW = 488; // 520 − 2×16 padding
const DROPDOWN_WIDTH = 240;

describe("window configuration", () => {
  const conf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf-8"));
  const windows = Object.fromEntries(
    conf.app.windows.map((window: { label: string }) => [window.label, window]),
  );

  it("lets the main window shrink in height but not below the designed width", () => {
    expect(windows.popover).toMatchObject({
      width: 520,
      height: 620,
      minWidth: 520,
      minHeight: 360,
      resizable: true,
      preventOverflow: true,
    });
  });

  it("creates the main window hidden so the remembered size applies before it shows", () => {
    // panel::show_on_launch shows it from the setup hook (a launch without
    // --autostart); a login launch leaves it hidden until the tray opens it.
    expect(windows.popover.visible).toBe(false);
  });

  it("gives the settings window the same treatment", () => {
    expect(windows.settings).toMatchObject({
      width: 460,
      height: 620,
      minWidth: 460,
      minHeight: 360,
      resizable: true,
      preventOverflow: true,
    });
  });

  it("asks for a dark title bar on both windows, like the dark macOS panel and settings", () => {
    // Without it Windows' light theme draws a white title bar over the dark
    // content. The settings window's recovery builder (commands.rs) sets the
    // same theme.
    expect(windows.popover.theme).toBe("Dark");
    expect(windows.settings.theme).toBe("Dark");
    const commands = readFileSync("src-tauri/src/commands.rs", "utf-8");
    expect(commands).toContain(".theme(Some(tauri::Theme::Dark))");
  });
});

describe("filter dropdown position", () => {
  it("matches the old fixed-width placement at the default width", () => {
    // The old code hard-coded available = 488: button width 116, centers at
    // 58 / 182 / 306 / 430, clamped into [0, 248].
    expect([0, 1, 2, 3].map((index) => dropdownLeft(index, 4, DEFAULT_ROW))).toEqual([
      0, 62, 186, 248,
    ]);
  });

  it("stays centered under its button and inside the row when the window is wider", () => {
    const row = 900;
    const buttonWidth = (row - 3 * 8) / 4;
    for (const index of [0, 1, 2, 3]) {
      const left = dropdownLeft(index, 4, row);
      expect(left).toBeGreaterThanOrEqual(0);
      expect(left + DROPDOWN_WIDTH).toBeLessThanOrEqual(row);
      const center = index * (buttonWidth + 8) + buttonWidth / 2;
      if (center - DROPDOWN_WIDTH / 2 >= 0 && center + DROPDOWN_WIDTH / 2 <= row) {
        expect(left + DROPDOWN_WIDTH / 2).toBeCloseTo(center);
      }
    }
  });
});

describe("quota card row", () => {
  it("keeps the 240px cards and 8px gap at the default width", () => {
    expect(quotaCardStep(DEFAULT_ROW)).toBe(240 + 8);
  });

  it("widens both cards with the window so the pair still fills the row", () => {
    const row = 788;
    const card = quotaCardStep(row) - 8;
    expect(card * 2 + 8).toBe(row);
  });
});
