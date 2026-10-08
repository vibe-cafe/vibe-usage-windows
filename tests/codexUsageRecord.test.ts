import { execFileSync } from "node:child_process";
import { expect, it } from "vitest";
it("supports durable Codex usage without duplicate UI mirrors, fork replay or cache drift", () => {
  const output = execFileSync(process.execPath, ["--test", "tests/run-codex-usage-record.mjs"], {encoding:"utf8", timeout:60_000});
  expect(output).toMatch(/fail 0/);
});
