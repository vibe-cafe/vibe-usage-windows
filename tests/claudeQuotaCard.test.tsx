import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { ProviderCard } from "../src/components/RateLimitCard";
import type { ProviderRateLimit } from "../src/lib/types";

vi.mock("../src/state/AppStateContext", () => ({
  useAppState: () => ({ quotaProducts: [], settings: {} }),
}));

function card(fields: Partial<ProviderRateLimit>) {
  return renderToStaticMarkup(
    <ProviderCard snapshot={{ provider: "claudeCode", status: { kind: "ok" }, ...fields }} />,
  );
}

const fable = { id: "Fable", label: "Fable", utilization: 12, windowDuration: 604800 };

it("shows only the general windows on the classic Claude card", () => {
  const html = card({
    fiveHour: { utilization: 19 },
    sevenDay: { utilization: 47 },
    modelScopedMeters: [fable],
  });
  expect(html).toMatch(/>5h<.*>19%<.*>7d<.*>47%</);
  expect(html).not.toMatch(/Fable|>12%<|另有/);
});

it("a weekly-only Pro account has exactly its general week", () => {
  const html = card({ planLabel: "Pro", sevenDay: { utilization: 47 }, modelScopedMeters: [fable] });
  expect(html).toMatch(/>7d<.*>47%</);
  expect(html).not.toMatch(/>5h<|Fable|另有/);
});
