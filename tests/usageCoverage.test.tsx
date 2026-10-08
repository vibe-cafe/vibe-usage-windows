import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { UsageCoverage } from "../src/components/UsageCoverage";
import { emptyFilters } from "../src/lib/types";
const fixture = vi.hoisted(() => ({ state: {} as Record<string, unknown> }));
vi.mock("../src/state/AppStateContext", () => ({ useAppState: () => fixture.state }));
describe("usage coverage", () => {
  it("identifies account-wide totals without treating buckets as separate devices", () => {
    fixture.state = { buckets: [{hostname:"win"}, {hostname:"mac"}, {hostname:"win"}], filters: emptyFilters() };
    const html = renderToStaticMarkup(<UsageCoverage />);
    expect(html).toContain("全部终端（2 台有数据）");
    expect(html).toContain("不是订阅账单或剩余额度");
  });
  it("makes active device filters and last successful cloud refresh explicit", () => {
    const filters = emptyFilters();
    filters.hostnames.add("mac");
    fixture.state = { buckets: [{hostname:"win"}, {hostname:"mac"}], filters, usageUpdatedAt: 1800000000000 };
    const html = renderToStaticMarkup(<UsageCoverage />);
    expect(html).toContain("已选 1 个终端");
    expect(html).toContain("清除全部筛选");
    expect(html).toContain("云端数据更新于");
  });
});
