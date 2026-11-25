import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import { observeCachedRateLimits } from "../src/lib/api";
import type { ProviderRateLimit } from "../src/lib/types";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});

it("only reads memory on open and reopen, and consumes refresh events without invoking a command", async () => {
  vi.stubGlobal("window", { crypto: globalThis.crypto });
  const old: ProviderRateLimit[] = [{ provider: "grok", status: { kind: "noData" } }];
  const fresh: ProviderRateLimit[] = [{ provider: "grok", status: { kind: "noData" }, emptyReason: "periodEnded" }];
  const reply = { snapshots: old, refreshing: true };
  let finishReopen!: (value: typeof reply) => void;
  const invoke = vi.fn().mockResolvedValueOnce(reply).mockImplementationOnce(
    () => new Promise<typeof reply>((resolve) => { finishReopen = resolve; }),
  );
  mockIPC(invoke, { shouldMockEvents: true });
  const update = vi.fn();
  const stop = observeCachedRateLimits(update);
  await vi.waitFor(() => expect(update).toHaveBeenCalledWith(null));
  await emit("rate-limits-updated", old);
  expect(update).toHaveBeenLastCalledWith(old);
  await emit("settings-shown");
  await emit("rate-limits-updated", null);
  expect(update).toHaveBeenLastCalledWith(null);
  await emit("rate-limits-updated", fresh);
  finishReopen(reply);
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(update).toHaveBeenLastCalledWith(fresh);
  expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual(["get_cached_rate_limits", "get_cached_rate_limits"]);
  stop();
  await emit("rate-limits-updated", []);
  expect(update).toHaveBeenCalledTimes(4);
});
