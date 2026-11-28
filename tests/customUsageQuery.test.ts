import { afterEach, expect, it, vi } from "vitest";
import { customUsageQuery } from "../src/lib/formatters";

afterEach(() => vi.unstubAllEnvs());

it.each([
  ["Asia/Shanghai", "2026-11-03", "2026-11-18", "2026-11-02T16:00:00.000Z", "2026-11-18T15:59:59.999Z"],
  ["Asia/Shanghai", "2026-11-03", "2026-11-03", "2026-11-02T16:00:00.000Z", "2026-11-03T15:59:59.999Z"],
  ["America/New_York", "2026-03-08", "2026-03-08", "2026-03-08T05:00:00.000Z", "2026-03-09T03:59:59.999Z"],
  ["America/New_York", "2026-11-01", "2026-11-01", "2026-11-01T04:00:00.000Z", "2026-11-02T04:59:59.999Z"],
])("requests the full local days in %s, %s through %s", (tz, first, last, fromIso, toIso) => {
  vi.stubEnv("TZ", tz);
  const from = new Date(first + "T12:00:00");
  const to = new Date(last + "T18:00:00");
  const expected = { kind: "custom", fromIso, toIso, tz };
  expect(customUsageQuery(from, to)).toEqual(expected);
  expect(customUsageQuery(to, from)).toEqual(expected);
});
