import { describe, expect, it } from "vitest";
import { groupModelsByFamily } from "../src/lib/modelFamilies";

describe("groupModelsByFamily", () => {
  it("groups by prefix with provider-prefix handling", () => {
    const groups = groupModelsByFamily([
      "claude-sonnet-4",
      "anthropic/claude-opus-4",
      "gpt-5.2",
      "codex-mini",
      "o3-mini",
      "gemini-2.5-pro",
      "deepseek-v3",
      "qwen3-coder",
      "glm-4.7",
      "kimi-k2",
      "moonshot-v1",
      "minimax-m2",
      "doubao-pro",
      "some-unknown-model",
      "ollama-local",
    ]);

    const byLabel = Object.fromEntries(groups.map((g) => [g.family?.label ?? "其他", g.models]));
    expect(byLabel["Claude"]).toEqual(["claude-sonnet-4", "anthropic/claude-opus-4"]);
    expect(byLabel["GPT"]).toEqual(["gpt-5.2", "codex-mini"]);
    expect(byLabel["o系列"]).toEqual(["o3-mini"]);
    expect(byLabel["Gemini"]).toEqual(["gemini-2.5-pro"]);
    expect(byLabel["DeepSeek"]).toEqual(["deepseek-v3"]);
    expect(byLabel["Qwen"]).toEqual(["qwen3-coder"]);
    expect(byLabel["GLM"]).toEqual(["glm-4.7"]);
    expect(byLabel["Kimi"]).toEqual(["kimi-k2", "moonshot-v1"]);
    expect(byLabel["MiniMax"]).toEqual(["minimax-m2"]);
    expect(byLabel["Doubao"]).toEqual(["doubao-pro"]);
    expect(byLabel["其他"]).toEqual(["some-unknown-model", "ollama-local"]);
  });

  it("o-family requires digit after o", () => {
    const groups = groupModelsByFamily(["o3", "opus-like", "ollama"]);
    const oGroup = groups.find((g) => g.family?.key === "o");
    expect(oGroup?.models).toEqual(["o3"]);
    const others = groups.find((g) => g.family === null);
    expect(others?.models).toEqual(["opus-like", "ollama"]);
  });

  it("families come in canonical order, 其他 last, empty families dropped", () => {
    const groups = groupModelsByFamily(["zzz", "gpt-4", "claude-3"]);
    expect(groups.map((g) => g.family?.label ?? "其他")).toEqual(["Claude", "GPT", "其他"]);
  });
});
