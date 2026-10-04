import { afterEach, describe, expect, it, vi } from "vitest";
import { startLiveRefresh } from "../src/lib/liveRefresh";
afterEach(() => vi.useRealTimers());
describe("visible panel refresh", () => {
  it("polls repeatedly, skips hidden panels and stops on disposal", async () => {
    vi.useFakeTimers();
    const refresh = vi.fn().mockResolvedValue(undefined);
    const visible = vi.fn().mockResolvedValue(true);
    const stop = startLiveRefresh(refresh, visible);
    await vi.advanceTimersByTimeAsync(120_000);
    expect(refresh).toHaveBeenCalledTimes(2);
    visible.mockResolvedValue(false);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(refresh).toHaveBeenCalledTimes(2);
    visible.mockResolvedValue(true);
    stop();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(refresh).toHaveBeenCalledTimes(2);
  });
  it("does not overlap slow refreshes and recovers from rejection", async () => {
    vi.useFakeTimers();
    let reject!: (e: Error) => void;
    const refresh = vi.fn().mockImplementationOnce(() => new Promise((_, r) => { reject = r; })).mockResolvedValue(undefined);
    const stop = startLiveRefresh(refresh, async () => true);
    await vi.advanceTimersByTimeAsync(180_000);
    expect(refresh).toHaveBeenCalledTimes(1);
    reject(new Error("offline"));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(refresh).toHaveBeenCalledTimes(2);
    stop();
  });
  it("does not refresh if disposed during the visibility check", async () => {
    vi.useFakeTimers();
    let resolve!: (v: boolean) => void;
    const refresh = vi.fn();
    const stop = startLiveRefresh(refresh, () => new Promise(r => { resolve = r; }));
    await vi.advanceTimersByTimeAsync(60_000);
    stop();
    resolve(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(refresh).not.toHaveBeenCalled();
  });
});
