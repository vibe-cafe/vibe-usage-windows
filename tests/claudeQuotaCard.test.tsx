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

it("shows the reported weekly model after the general week, including zero usage", () => {
  const html = card({
    sevenDay: { utilization: 47 },
    modelScopedMeters: [{ id: "Fable", label: "Fable", utilization: 0, windowDuration: 604800 }],
  });
  expect(html).toMatch(/>7d<.*>47%<.*>Fable<.*>0%</);
  expect(html).not.toContain(">5h<");
});

it("a weekly-only Pro account has exactly its general week", () => {
  const html = card({ planLabel: "Pro", sevenDay: { utilization: 47 } });
  expect(html).toMatch(/>7d<.*>47%</);
  expect(html).not.toMatch(/>5h<|Fable|另有/);
});
