import { describe, expect, it } from "vitest";
import {
  aggregateSlices,
  buildChartData,
  barTotal,
  elapsedPercent,
  filterBuckets,
  filterSessions,
  labelInterval,
  summarize,
  utilizationColor,
} from "../src/lib/aggregate";
import { emptyFilters, UsageBucket, UsageSession } from "../src/lib/types";

function bucket(overrides: Partial<UsageBucket> = {}): UsageBucket {
  return {
    source: "claude-code",
    model: "claude-sonnet-4",
    project: "proj-a",
    hostname: "pc-1",
    bucketStart: "2026-07-03T04:00:00.000Z",
    inputTokens: 100,
    outputTokens: 50,
    cachedInputTokens: 200,
    reasoningOutputTokens: 10,
    totalTokens: 360,
    estimatedCost: 1.5,
    ...overrides,
  };
}

function session(overrides: Partial<UsageSession> = {}): UsageSession {
  return {
    source: "claude-code",
    project: "proj-a",
    hostname: "pc-1",
    firstMessageAt: "2026-07-03T04:00:00.000Z",
    lastMessageAt: "2026-07-03T05:00:00.000Z",
    durationSeconds: 3600,
    activeSeconds: 900,
    messageCount: 10,
    userMessageCount: 3,
    ...overrides,
  };
}

describe("filterBuckets", () => {
  it("applies 4-dimension filters", () => {
    const buckets = [
      bucket(),
      bucket({ source: "codex" }),
      bucket({ model: "gpt-5" }),
      bucket({ hostname: "pc-2" }),
    ];
    const f = emptyFilters();
    f.sources.add("claude-code");
    expect(filterBuckets(buckets, f, "7D")).toHaveLength(3);
    f.models.add("claude-sonnet-4");
    expect(filterBuckets(buckets, f, "7D")).toHaveLength(2);
    f.hostnames.add("pc-1");
    expect(filterBuckets(buckets, f, "7D")).toHaveLength(1);
  });

  it("today cutoff drops earlier buckets", () => {
    const now = new Date("2026-07-03T20:00:00");
    const todayLocalIso = new Date("2026-07-03T10:00:00").toISOString();
    const yesterdayIso = new Date("2026-07-02T10:00:00").toISOString();
    const buckets = [bucket({ bucketStart: todayLocalIso }), bucket({ bucketStart: yesterdayIso })];
    expect(filterBuckets(buckets, emptyFilters(), "today", now)).toHaveLength(1);
    expect(filterBuckets(buckets, emptyFilters(), "1D", now)).toHaveLength(2);
  });
});

describe("filterSessions", () => {
  it("model filter does NOT apply to sessions (macOS behavior)", () => {
    const f = emptyFilters();
    f.models.add("some-model-nothing-matches");
    expect(filterSessions([session()], f, "7D")).toHaveLength(1);
  });

  it("source/project/hostname filters do apply", () => {
    const f = emptyFilters();
    f.sources.add("codex");
    expect(filterSessions([session()], f, "7D")).toHaveLength(0);
  });
});

describe("summarize", () => {
  it("computedTotal = input+output+reasoning+cached; cache separate; active from sessions", () => {
    const totals = summarize([bucket(), bucket({ estimatedCost: null })], [session(), session()]);
    expect(totals.totalCost).toBe(1.5);
    expect(totals.totalTokens).toBe(2 * (100 + 50 + 10 + 200));
    expect(totals.totalCachedInputTokens).toBe(400);
    expect(totals.totalActiveSeconds).toBe(1800);
  });
});

describe("buildChartData", () => {
  it("daily mode fills every slot and aggregates by UTC dayKey", () => {
    const now = new Date("2026-07-03T12:00:00");
    const data = buildChartData(
      [bucket({ bucketStart: "2026-07-01T04:00:00.000Z", inputTokens: 7 })],
      [session({ firstMessageAt: "2026-07-01T04:00:00.000Z", activeSeconds: 600 })],
      "7D",
      7,
      null,
      now,
    );
    expect(data).toHaveLength(7);
    const day = data.find((d) => d.id === "2026-07-01")!;
    expect(day.input).toBe(7);
    expect(day.activeMinutes).toBe(10);
    // reasoning folded into output
    expect(day.output).toBe(50 + 10);
    expect(barTotal(day)).toBe(7 + 60 + 200);
    // empty slots exist with zeros
    expect(data.filter((d) => barTotal(d) === 0).length).toBe(6);
  });

  it("24H mode produces a 24-slot rolling window; today grows from midnight", () => {
    const now = new Date("2026-07-03T05:30:00");
    const rolling = buildChartData([], [], "1D", 1, null, now);
    expect(rolling).toHaveLength(24);
    const today = buildChartData([], [], "today", 1, null, now);
    expect(today).toHaveLength(6); // 00,01,02,03,04,05
  });

  it("custom range ends at custom `to`", () => {
    const now = new Date("2026-07-03T12:00:00");
    const to = new Date("2026-06-30T00:00:00");
    const data = buildChartData([], [], "custom", 3, to, now);
    expect(data.map((d) => d.id)).toEqual(["2026-06-28", "2026-06-29", "2026-06-30"]);
  });
});

describe("labelInterval", () => {
  it("mirrors BarChartView tiers", () => {
    expect(labelInterval(12, true)).toBe(3);
    expect(labelInterval(18, true)).toBe(4);
    expect(labelInterval(24, true)).toBe(6);
    expect(labelInterval(7, false)).toBe(1);
    expect(labelInterval(15, false)).toBe(3);
    expect(labelInterval(30, false)).toBe(7);
    expect(labelInterval(90, false)).toBe(14);
    expect(labelInterval(365, false)).toBe(30);
  });
});

describe("aggregateSlices", () => {
  it("top-6 + 其他, sorted by tokens, empty label → 未知", () => {
    const buckets = Array.from({ length: 8 }, (_, i) =>
      bucket({
        hostname: `pc-${i}`,
        inputTokens: (8 - i) * 100,
        outputTokens: 0,
        cachedInputTokens: 0,
        reasoningOutputTokens: 0,
      }),
    );
    buckets.push(bucket({ hostname: "", inputTokens: 5000, outputTokens: 0, cachedInputTokens: 0, reasoningOutputTokens: 0 }));

    const slices = aggregateSlices(buckets, (b) => b.hostname);
    expect(slices[0].label).toBe("未知");
    expect(slices).toHaveLength(7); // 6 + 其他
    expect(slices[6].label).toBe("其他");
    expect(slices[6].tokens).toBe(300 + 200 + 100); // three smallest overflow into 其他
  });
});

describe("rate-limit helpers", () => {
  it("elapsedPercent from resetsAt + duration", () => {
    const now = new Date("2026-07-03T12:00:00Z");
    const w = {
      utilization: 50,
      resetsAt: now.getTime() / 1000 + 3600, // 1h remaining
      windowDuration: 5 * 3600,
    };
    expect(elapsedPercent(w, now)).toBeCloseTo(80, 5);
    expect(elapsedPercent({ utilization: 50 }, now)).toBeNull();
  });

  it("utilizationColor tiers", () => {
    expect(utilizationColor(10)).toBe("#D9D9D9");
    expect(utilizationColor(70)).toBe("#F59E0B");
    expect(utilizationColor(90)).toBe("#F04545");
  });
});
