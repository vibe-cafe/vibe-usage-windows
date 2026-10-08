import { describe, expect, it } from "vitest";
import { filterBuckets } from "../src/lib/aggregate";
import {
  UsageBucket,
  UsageNames,
  emptyFilters,
  modelFilterGroups,
  modelName,
  toolName,
} from "../src/lib/types";

// Replaces the deleted modelFamilies test: the family table now comes from the
// server, so what is worth pinning is that this app reads it and keeps none of
// its own.

function bucket(overrides: Partial<UsageBucket> = {}): UsageBucket {
  return {
    source: "kimi-code",
    model: "k3",
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

const names: UsageNames = {
  sources: { "kimi-code": "Kimi Code", opencode: "OpenCode" },
  models: { k3: "Kimi K3", "kimi-code/k3-256k": "Kimi K3" },
  modelFamilies: { k3: "kimi", "kimi-code/k3-256k": "kimi" },
  families: [{ key: "kimi", label: "Kimi", provider: "Moonshot AI" }],
};

describe("display names come from the server", () => {
  it("uses the supplied name and falls back to the raw id", () => {
    expect(toolName(names, "kimi-code")).toBe("Kimi Code");
    expect(modelName(names, "k3")).toBe("Kimi K3");
    // Unresolved ids render exactly as reported — never a guess.
    expect(toolName(names, "some-new-cli")).toBe("some-new-cli");
    expect(modelName(names, "acme-coder-2.5")).toBe("acme-coder-2.5");
    expect(toolName(null, "kimi-code")).toBe("kimi-code");
    expect(modelName(undefined, "k3")).toBe("k3");
  });

  it("matches a model filter on the display name, so one name selects every id behind it", () => {
    const buckets = [bucket(), bucket({ model: "kimi-code/k3-256k", source: "opencode" })];
    const filters = { ...emptyFilters(), models: new Set(["Kimi K3"]) };
    expect(filterBuckets(buckets, filters, "30D", names)).toHaveLength(2);

    const miss = { ...emptyFilters(), models: new Set(["Kimi K2"]) };
    expect(filterBuckets(buckets, miss, "30D", names)).toHaveLength(0);
  });

  it("groups the model options by the server's family rows", () => {
    const buckets = [bucket(), bucket({ model: "kimi-code/k3-256k" })];
    const groups = modelFilterGroups(buckets, names);
    expect(groups).toEqual([{ key: "kimi", label: "Kimi", models: ["Kimi K3"] }]);
  });

  it("puts names the server could not place into 其他", () => {
    // A model the server neither named nor placed; the merged pair above still
    // resolves through the raw id that does have a family.
    const buckets = [bucket(), bucket({ model: "kimi-code/k3-256k" }), bucket({ model: "acme-coder-2.5" })];
    const groups = modelFilterGroups(buckets, names);
    expect(groups.map((g) => g.key)).toEqual(["kimi", "other"]);
    expect(groups.at(-1)?.models).toEqual(["acme-coder-2.5"]);
  });

  it("without server names everything is unplaced and ids stay raw", () => {
    const buckets = [bucket(), bucket({ model: "gpt-6" })];
    expect(modelFilterGroups(buckets, null)).toEqual([
      { key: "other", label: "其他", models: ["gpt-6", "k3"] },
    ]);
    expect(filterBuckets(buckets, emptyFilters(), "30D", null)).toHaveLength(2);
  });
});
