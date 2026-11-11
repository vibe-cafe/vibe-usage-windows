/**
 * The shipped launcher (`src-tauri/resources/cli-bootstrap.mjs`) run as the app
 * runs it — a separate Node process — with `fetch` replaced by a preloaded stub
 * (`--import`), so every registry answer and failure is reproducible offline
 * and the launcher itself carries no test hook. Cached "CLI versions" are tiny
 * fake packages whose `run()` only prints which version ran.
 */
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LAUNCHER = join(ROOT, "src-tauri", "resources", "cli-bootstrap.mjs");
const NOTICE = "vibe-usage launcher notice:";
const FAILURE = "vibe-usage launcher:";

/** Answers the launcher's two requests from `BOOTSTRAP_TEST_FETCH`. */
const STUB = `
const plan = JSON.parse(process.env.BOOTSTRAP_TEST_FETCH);
const networkError = () =>
  new TypeError("fetch failed", { cause: Object.assign(new Error("connect ETIMEDOUT 104.16.0.35:443"), { code: "ETIMEDOUT" }) });
const answer = (kind) => {
  switch (kind.type) {
    case "network": throw networkError();
    case "timeout": throw new DOMException("The operation was aborted due to timeout", "TimeoutError");
    case "status": return new Response("", { status: kind.status });
    case "body": return new Response(kind.body, { status: 200 });
    case "json": return Response.json(kind.json);
  }
  throw new Error("unplanned request");
};
globalThis.fetch = async (url) => {
  const target = String(url);
  if (target.endsWith("/latest")) return answer(plan.latest);
  process.stderr.write("stub: tarball requested\\n");
  return answer(plan.tarball ?? { type: "network" });
};
`;

let home: string;
let stubUrl: string;

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), "vu-bootstrap-"));
  const stub = join(home, "fetch-stub.mjs");
  writeFileSync(stub, STUB);
  stubUrl = pathToFileURL(stub).href;
});

afterEach(() => {
  rmSync(home, { recursive: true, force: true });
});

interface FakeVersion {
  complete?: boolean;
  entry?: boolean;
  /** `run()` throws after printing, like a CLI that crashes. */
  crashes?: boolean;
}

/** A cache directory the way the launcher leaves it after an install. */
function cache(versions: Record<string, FakeVersion>) {
  for (const [name, { complete = true, entry = true, crashes = false }] of Object.entries(versions)) {
    const dir = join(home, "cli-cache", name);
    mkdirSync(join(dir, "package", "src"), { recursive: true });
    writeFileSync(join(dir, "package", "package.json"), JSON.stringify({ name: "fake", type: "module" }));
    if (entry) {
      writeFileSync(
        join(dir, "package", "src", "index.js"),
        `export async function run(argv) {
  process.stdout.write("ran ${name} " + argv.join(" ") + "\\n");
  ${crashes ? 'throw new Error("cli crashed");' : ""}
}
`,
      );
    }
    if (complete) writeFileSync(join(dir, "complete"), name);
  }
}

type Answer =
  | { type: "network" }
  | { type: "timeout" }
  | { type: "status"; status: number }
  | { type: "body"; body: string }
  | { type: "json"; json: unknown };

function launch(plan: { latest: Answer; tarball?: Answer }) {
  const result = spawnSync(process.execPath, [`--import=${stubUrl}`, LAUNCHER, "sync"], {
    cwd: home,
    encoding: "utf8",
    timeout: 30_000,
    env: {
      ...process.env,
      VIBE_USAGE_CONFIG_DIR: home,
      BOOTSTRAP_TEST_FETCH: JSON.stringify(plan),
    },
  });
  const stderr = result.stderr.split(/\r?\n/).filter(Boolean);
  return {
    status: result.status,
    stdout: result.stdout,
    stderr,
    notices: stderr.filter((line) => line.startsWith(NOTICE)),
    failures: stderr.filter((line) => line.startsWith(FAILURE)),
  };
}

const latest = (version: string, extra: Record<string, string> = {}): Answer => ({
  type: "json",
  json: { version, dist: { tarball: `https://registry.npmjs.org/fake/-/fake-${version}.tgz`, ...extra } },
});

describe("cli-bootstrap: latest cannot be resolved", () => {
  // Every kind of half-finished or foreign entry a real cache can hold, around
  // the version that must win: numeric order (0.14.11 > 0.14.9 > 0.9.0), not
  // string order, and only directories with the marker *and* the entry file.
  const realisticCache = () =>
    cache({
      "0.9.0": {},
      "0.14.9": {},
      "0.14.11": {},
      "0.14.11-rc.1": {},
      "0.15.0": { complete: false },
      "0.15.1": { entry: false },
      ".staging-4242-1794205920000-abc": {},
      "not-a-version": {},
    });

  for (const [name, answer, stage] of [
    ["network error", { type: "network" }, "network"],
    ["timeout", { type: "timeout" }, "network"],
    ["registry error status", { type: "status", status: 503 }, "registry"],
    ["unreadable answer (captive portal page)", { type: "body", body: "<html>login</html>" }, "registry"],
    ["answer without version", { type: "json", json: { name: "@vibe-cafe/vibe-usage" } }, "registry"],
  ] as const) {
    it(`runs the newest complete cached version on ${name}`, () => {
      realisticCache();
      const run = launch({ latest: answer });
      expect(run.status).toBe(0);
      expect(run.stdout).toBe("ran 0.14.11 sync\n");
      expect(run.failures).toEqual([]);
      expect(run.notices).toHaveLength(1);
      expect(run.notices[0]).toMatch(
        new RegExp(`^${NOTICE} stage=${stage}; running cached @vibe-cafe/vibe-usage@0\\.14\\.11 `),
      );
    });
  }

  it("keeps the notice to one line with the network cause", () => {
    cache({ "0.14.11": {} });
    const run = launch({ latest: { type: "network" } });
    expect(run.stderr).toEqual([
      `${NOTICE} stage=network; running cached @vibe-cafe/vibe-usage@0.14.11 because latest is unavailable (fetch failed: ETIMEDOUT)`,
    ]);
  });

  it("fails exactly as before when nothing usable is cached", () => {
    cache({ "0.15.0": { complete: false }, "0.15.1": { entry: false } });
    const network = launch({ latest: { type: "network" } });
    expect(network.status).toBe(1);
    expect(network.stdout).toBe("");
    expect(network.notices).toEqual([]);
    expect(network.stderr[0]).toBe(`${FAILURE} TypeError: fetch failed`);

    const registry = launch({ latest: { type: "status", status: 503 } });
    expect(registry.status).toBe(1);
    expect(registry.stderr).toEqual([`${FAILURE} registry returned 503 for @vibe-cafe/vibe-usage@latest`]);
  });

  it("fails as before with no cache directory at all", () => {
    const run = launch({ latest: { type: "status", status: 404 } });
    expect(run.status).toBe(1);
    expect(run.stderr).toEqual([`${FAILURE} registry returned 404 for @vibe-cafe/vibe-usage@latest`]);
  });
});

describe("cli-bootstrap: latest resolves", () => {
  it("runs a cached latest without downloading or a notice", () => {
    cache({ "0.14.9": {}, "0.14.11": {} });
    const run = launch({ latest: latest("0.14.9") });
    expect(run.status).toBe(0);
    expect(run.stdout).toBe("ran 0.14.9 sync\n");
    expect(run.stderr).toEqual([]);
  });

  for (const [name, tarball, extra] of [
    ["the download fails", { type: "status", status: 404 }, {}],
    ["the download drops", { type: "network" }, {}],
    ["the download does not verify", { type: "body", body: "not the package" }, { integrity: "sha512-AAAA" }],
  ] as const) {
    it(`falls back to the cache when a new latest is out and ${name}`, () => {
      cache({ "0.14.11": {} });
      const run = launch({ latest: latest("0.16.0", extra), tarball });
      expect(run.status).toBe(0);
      expect(run.stdout).toBe("ran 0.14.11 sync\n");
      expect(run.failures).toEqual([]);
      expect(run.notices).toHaveLength(1);
      expect(run.notices[0]).toMatch(
        new RegExp(`^${NOTICE} stage=download; running cached @vibe-cafe/vibe-usage@0\\.14\\.11 `),
      );
    });
  }

  it("fails with the old download messages when nothing is cached", () => {
    const status = launch({ latest: latest("0.16.0"), tarball: { type: "status", status: 404 } });
    expect(status.status).toBe(1);
    expect(status.failures).toEqual([`${FAILURE} download failed (404) for @vibe-cafe/vibe-usage@0.16.0`]);

    const integrity = launch({
      latest: latest("0.16.0", { integrity: "sha512-AAAA" }),
      tarball: { type: "body", body: "not the package" },
    });
    expect(integrity.status).toBe(1);
    expect(integrity.failures).toEqual([`${FAILURE} integrity mismatch for @vibe-cafe/vibe-usage@0.16.0`]);
  });
});

describe("cli-bootstrap: the CLI itself runs once", () => {
  it("does not retry a crashing cached latest with another version", () => {
    cache({ "0.14.9": {}, "0.14.11": { crashes: true } });
    const run = launch({ latest: latest("0.14.11") });
    expect(run.status).toBe(1);
    expect(run.stdout).toBe("ran 0.14.11 sync\n");
    expect(run.notices).toEqual([]);
    expect(run.failures[0]).toBe(`${FAILURE} Error: cli crashed`);
  });

  it("does not retry a crashing fallback version", () => {
    cache({ "0.14.9": {}, "0.14.11": { crashes: true } });
    const run = launch({ latest: { type: "network" } });
    expect(run.status).toBe(1);
    expect(run.stdout).toBe("ran 0.14.11 sync\n");
    expect(run.notices).toHaveLength(1);
  });
});
