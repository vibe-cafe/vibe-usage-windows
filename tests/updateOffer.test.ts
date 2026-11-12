import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { followUpdateOffer, type UpdateOfferSource } from "../src/lib/updateOffer";
import type { UpdateInfo } from "../src/lib/types";

const offer = (version: string): UpdateInfo => ({
  version,
  url: `https://example.test/VibeUsage-${version}-Setup.exe`,
});

/** A stand-in for the two Tauri events plus the read-only command. */
function source(readCurrent: (events: Events) => Promise<UpdateInfo | null>) {
  const events: Events = { attached: 0, unlistened: 0, available: () => {}, cleared: () => {} };
  const src: UpdateOfferSource = {
    onAvailable: async (handler) => {
      events.available = handler;
      events.attached += 1;
      return () => void (events.unlistened += 1);
    },
    onCleared: async (handler) => {
      events.cleared = handler;
      events.attached += 1;
      return () => void (events.unlistened += 1);
    },
    readCurrent: () => readCurrent(events),
  };
  return { src, events };
}

interface Events {
  attached: number;
  unlistened: number;
  available: (info: UpdateInfo) => void;
  cleared: () => void;
}

describe("update offer at startup", () => {
  test("an update the backend found before the listeners were attached is still shown", async () => {
    const applied: Array<string | null> = [];
    const { src } = source(async (events) => {
      // Read only once both listeners are in place, so nothing falls in between.
      expect(events.attached).toBe(2);
      return offer("0.7.6");
    });
    await followUpdateOffer(src, (info) => applied.push(info?.version ?? null));
    expect(applied).toEqual(["0.7.6"]);
  });

  test("nothing found yet shows nothing, and later events still arrive", async () => {
    const applied: Array<string | null> = [];
    const { src, events } = source(async () => null);
    await followUpdateOffer(src, (info) => applied.push(info?.version ?? null));
    expect(applied).toEqual([]);
    events.available(offer("0.7.6"));
    events.cleared();
    expect(applied).toEqual(["0.7.6", null]);
  });

  test("an event that arrives while the read is in flight wins over the read", async () => {
    const applied: Array<string | null> = [];
    const { src } = source(async (events) => {
      events.available(offer("0.7.7"));
      return offer("0.7.6");
    });
    await followUpdateOffer(src, (info) => applied.push(info?.version ?? null));
    expect(applied).toEqual(["0.7.7"]);
  });

  test("a withdrawal during the read is not undone by the stale read (#36)", async () => {
    const applied: Array<string | null> = [];
    const { src } = source(async (events) => {
      events.cleared();
      return offer("0.7.6");
    });
    await followUpdateOffer(src, (info) => applied.push(info?.version ?? null));
    expect(applied).toEqual([null]);
  });

  test("a failed read leaves the listeners working, and unlisten stops both", async () => {
    const applied: Array<string | null> = [];
    const { src, events } = source(async () => {
      throw new Error("not ready");
    });
    const unlisten = await followUpdateOffer(src, (info) => applied.push(info?.version ?? null));
    events.available(offer("0.7.6"));
    expect(applied).toEqual(["0.7.6"]);
    unlisten();
    expect(events.unlistened).toBe(2);
  });

  test("the panel uses it, and the backend command only reads", () => {
    const appState = readFileSync("src/state/AppStateContext.tsx", "utf-8");
    const commands = readFileSync("src-tauri/src/commands.rs", "utf-8");
    const lib = readFileSync("src-tauri/src/lib.rs", "utf-8");
    expect(appState).toContain(
      "{ onAvailable: onUpdateAvailable, onCleared: onUpdateCleared, readCurrent: api.getUpdateInfo }",
    );
    expect(lib).toContain("commands::get_update_info,");
    const body = commands.slice(commands.indexOf("pub fn get_update_info"));
    expect(body.slice(0, body.indexOf("\n}\n"))).not.toContain("updater::check");
    // The check schedule is untouched (startup + every 24h).
    const scheduler = readFileSync("src-tauri/src/services/scheduler.rs", "utf-8");
    expect(scheduler).toContain("Background update poll (startup + every 24h).");
  });
});
