import { describe, expect, it } from "vitest";
import { EGRESS_MODES, egressModeOption } from "../src/lib/egressMode";

describe("the 「没检测到代理时」 menu", () => {
  it("offers the three options in the agreed words, the default first", () => {
    expect(EGRESS_MODES.map((mode) => [mode.value, mode.label])).toEqual([
      ["auto", "自动判断（推荐）"],
      ["pause", "暂停读取"],
      ["allow", "照常读取"],
    ]);
  });

  it("gives each option its own short grey line", () => {
    expect(egressModeOption("auto").caption).toBe(
      "网络在中国大陆等地区时，暂停读取 Codex、Claude、Grok、Antigravity，避免真实 IP 和账号一起暴露。",
    );
    expect(egressModeOption("pause").caption).toContain("检测到代理后自动继续");
    expect(egressModeOption("allow").caption).toContain("真实 IP");
  });

  it("reads anything unknown as the default", () => {
    expect(egressModeOption(undefined).value).toBe("auto");
    expect(egressModeOption("somethingNew").value).toBe("auto");
  });
});
