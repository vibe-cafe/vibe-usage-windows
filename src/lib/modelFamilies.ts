// 1:1 port of VibeUsage/Models/ModelFamilies.swift

export interface ModelFamily {
  key: string;
  label: string;
  matches: (id: string) => boolean;
}

export const MODEL_FAMILIES: ModelFamily[] = [
  { key: "claude", label: "Claude", matches: (id) => id.startsWith("claude") },
  { key: "gpt", label: "GPT", matches: (id) => id.startsWith("gpt") || id.startsWith("codex") },
  {
    key: "o",
    label: "o系列",
    matches: (id) => id.length > 1 && id[0] === "o" && id[1] >= "0" && id[1] <= "9",
  },
  { key: "gemini", label: "Gemini", matches: (id) => id.startsWith("gemini") },
  { key: "deepseek", label: "DeepSeek", matches: (id) => id.startsWith("deepseek") },
  { key: "qwen", label: "Qwen", matches: (id) => id.startsWith("qwen") },
  { key: "glm", label: "GLM", matches: (id) => id.startsWith("glm") },
  {
    key: "kimi",
    label: "Kimi",
    matches: (id) => id.startsWith("kimi") || id.startsWith("moonshot"),
  },
  { key: "minimax", label: "MiniMax", matches: (id) => id.startsWith("minimax") },
  { key: "doubao", label: "Doubao", matches: (id) => id.startsWith("doubao") },
];

export interface ModelGroup {
  family: ModelFamily | null;
  models: string[];
}

export function groupModelsByFamily(models: string[]): ModelGroup[] {
  const familyMap = new Map<string, string[]>();
  const others: string[] = [];

  for (const family of MODEL_FAMILIES) {
    familyMap.set(family.key, []);
  }

  for (const model of models) {
    const lower = model.toLowerCase();
    // Handle provider prefixes like "anthropic/claude-opus-4-20250514"
    const slashIndex = lower.indexOf("/");
    const base = slashIndex >= 0 ? lower.slice(slashIndex + 1) : lower;

    const family = MODEL_FAMILIES.find((f) => f.matches(base));
    if (family) {
      familyMap.get(family.key)!.push(model);
    } else {
      others.push(model);
    }
  }

  const result: ModelGroup[] = [];
  for (const family of MODEL_FAMILIES) {
    const familyModels = familyMap.get(family.key) ?? [];
    if (familyModels.length > 0) {
      result.push({ family, models: familyModels });
    }
  }
  if (others.length > 0) {
    result.push({ family: null, models: others });
  }
  return result;
}
