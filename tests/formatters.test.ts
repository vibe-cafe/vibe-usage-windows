// Cases translated 1:1 from Formatters.swift semantics.
import { describe, expect, it } from "vitest";
import {
  formatCost,
  formatDateShort,
  formatDuration,
  formatNumber,
  formatPercent,
  formatRelativeTime,
  formatSlicePercent,
  formatTimeUntil,
  formatCenterTokens,
  localDayKey,
} from "../src/lib/formatters";

describe("formatNumber", () => {
  it("compact notation", () => {
    expect(formatNumber(0)).toBe("0");
    expect(formatNumber(1234)).toBe("1,234");
    expect(formatNumber(9999)).toBe("9,999");
    expect(formatNumber(10_000)).toBe("10.0K");
    expect(formatNumber(45_200)).toBe("45.2K");
    expect(formatNumber(999_999)).toBe("1000.0K");
    expect(formatNumber(1_000_000)).toBe("1.0M");
    expect(formatNumber(196_600_000)).toBe("196.6M");
  });
});

describe("formatCost", () => {
  it("three tiers", () => {
    expect(formatCost(0)).toBe("$0.00");
    expect(formatCost(0.0012)).toBe("$0.0012");
    expect(formatCost(0.009999)).toBe("$0.0100");
    expect(formatCost(0.01)).toBe("$0.01");
    expect(formatCost(12.34)).toBe("$12.34");
    expect(formatCost(117.07)).toBe("$117.07");
  });
});

describe("formatDateShort", () => {
  it("strips leading zeros", () => {
    expect(formatDateShort("2026-02-25")).toBe("2/25");
    expect(formatDateShort("2026-12-05")).toBe("12/5");
    expect(formatDateShort("garbage")).toBe("garbage");
  });
});

describe("formatRelativeTime", () => {
  const now = new Date("2026-07-03T12:00:00");
  it("tiers", () => {
    expect(formatRelativeTime(new Date("2026-07-03T11:59:30"), now)).toBe("刚刚");
    expect(formatRelativeTime(new Date("2026-07-03T11:57:00"), now)).toBe("3 分钟前");
    expect(formatRelativeTime(new Date("2026-07-03T11:00:00"), now)).toBe("1 小时前");
    expect(formatRelativeTime(new Date("2026-07-01T12:00:00"), now)).toBe("2 天前");
  });
});

describe("formatDuration", () => {
  it("matches Swift tiers", () => {
    expect(formatDuration(0)).toBe("0m");
    expect(formatDuration(-5)).toBe("0m");
    expect(formatDuration(30)).toBe("1m"); // sub-minute clamps to 1m
    expect(formatDuration(90)).toBe("1m");
    expect(formatDuration(3600)).toBe("1h");
    expect(formatDuration(3661)).toBe("1h 1m");
    expect(formatDuration(5340)).toBe("1h 29m");
    expect(formatDuration(86400)).toBe("1d");
    expect(formatDuration(93600)).toBe("1d 2h");
  });
});

describe("formatTimeUntil", () => {
  const now = new Date("2026-07-03T12:00:00");
  it("future and past", () => {
    expect(formatTimeUntil(new Date("2026-07-03T12:12:00"), now)).toBe("12m");
    expect(formatTimeUntil(new Date("2026-07-03T14:14:00"), now)).toBe("2h 14m");
    expect(formatTimeUntil(new Date("2026-07-03T11:00:00"), now)).toBe("已重置");
  });
});

describe("formatPercent (rate-limit rows)", () => {
  it("mirrors percentText", () => {
    expect(formatPercent(0.01)).toBe("0%");
    expect(formatPercent(0.5)).toBe("0.5%");
    expect(formatPercent(14)).toBe("14%");
    expect(formatPercent(88.6)).toBe("89%");
  });
});

describe("formatSlicePercent", () => {
  it("mirrors donut legend", () => {
    expect(formatSlicePercent(0, 0)).toBe("0%");
    expect(formatSlicePercent(1, 2000)).toBe("<0.1%");
    expect(formatSlicePercent(871, 1000)).toBe("87.1%");
  });
});

describe("formatCenterTokens", () => {
  it("adds billions tier", () => {
    expect(formatCenterTokens(950)).toBe("950");
    expect(formatCenterTokens(10_100_000)).toBe("10.1M");
    expect(formatCenterTokens(1_200_000_000)).toBe("1.2B");
  });
});

describe("localDayKey", () => {
  it("formats local date", () => {
    expect(localDayKey(new Date(2026, 6, 3))).toBe("2026-07-03");
    expect(localDayKey(new Date(2026, 0, 9))).toBe("2026-01-09");
  });
});
