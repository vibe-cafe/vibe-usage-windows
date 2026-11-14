/**
 * The shipped launcher (`src-tauri/resources/cli-bootstrap.mjs`) run as the app
 * runs it — a separate Node process — with `fetch` replaced by a preloaded stub
 * (`--import`), so every registry answer and failure is reproducible offline
 * and the launcher itself carries no test hook. Cached "CLI versions" are tiny
 * fake packages whose `run()` only prints which version ran.
 */
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LAUNCHER = join(ROOT, "src-tauri", "resources", "cli-bootstrap.mjs");
const NOTICE = "vibe-usage launcher notice:";
const FAILURE = "vibe-usage launcher:";

/**
 * Answers the launcher's two requests from `BOOTSTRAP_TEST_FETCH`, and reports
 * on stderr (`stub: …` lines, split off by `launch`) each tarball request and
 * each timeout the launcher asks for, every URL it fetches (`url: …`), and each
 * request's options as JSON (`req: …`: header pairs and option names).
 */
const STUB = `
const plan = JSON.parse(process.env.BOOTSTRAP_TEST_FETCH);
const timeout = AbortSignal.timeout.bind(AbortSignal);
AbortSignal.timeout = (ms) => {
  process.stderr.write("stub: timeout " + ms + "\\n");
  return timeout(ms);
};
const fetchUrl = (url) => process.stderr.write("url: " + url + "\\n");
const fetchRequest = (url, init) => process.stderr.write("req: " + JSON.stringify({
  url,
  headers: [...new Headers(init?.headers ?? {}).entries()],
  options: Object.keys(init ?? {}),
}) + "\\n");
const networkError = () =>
  new TypeError("fetch failed", { cause: Object.assign(new Error("connect ETIMEDOUT 104.16.0.35:443"), { code: "ETIMEDOUT" }) });
const answer = (kind) => {
  switch (kind.type) {
    case "network": throw networkError();
    case "timeout": throw new DOMException("The operation was aborted due to timeout", "TimeoutError");
    case "status": return new Response("", { status: kind.status });
    case "body": return new Response(kind.body, { status: 200 });
    case "bytes": return new Response(Buffer.from(kind.base64, "base64"), { status: 200 });
    case "json": return Response.json(kind.json);
  }
  throw new Error("unplanned request");
};
globalThis.fetch = async (url, init) => {
  const target = String(url);
  fetchUrl(target);
  fetchRequest(target, init);
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
  /** The entry module throws while loading, before `run()` exists. */
  brokenImport?: boolean;
}

/** A cache directory the way the launcher leaves it after an install. */
function cache(versions: Record<string, FakeVersion>) {
  for (const [name, { complete = true, entry = true, crashes = false, brokenImport = false }] of Object.entries(versions)) {
    const dir = join(home, "cli-cache", name);
    mkdirSync(join(dir, "package", "src"), { recursive: true });
    writeFileSync(join(dir, "package", "package.json"), JSON.stringify({ name: "fake", type: "module" }));
    if (entry) {
      writeFileSync(
        join(dir, "package", "src", "index.js"),
        `${brokenImport ? 'throw new Error("cli failed to load");' : ""}
export async function run(argv) {
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
  | { type: "bytes"; base64: string }
  | { type: "json"; json: unknown };

function launch(plan: { latest: Answer; tarball?: Answer }) {
  const result = spawnSync(process.execPath, [`--import=${stubUrl}`, LAUNCHER, "sync"], {
    cwd: home,
    encoding: "utf8",
    timeout: 30_000,
    env: {
      ...process.env,
      // `os.homedir()` for `~/.npmrc`: never the developer's or the runner's own.
      HOME: home,
      USERPROFILE: home,
      VIBE_USAGE_CONFIG_DIR: home,
      BOOTSTRAP_TEST_FETCH: JSON.stringify(plan),
    },
  });
  const lines = result.stderr.split(/\r?\n/).filter(Boolean);
  const stderr = lines.filter(
    (line) => !line.startsWith("stub: ") && !line.startsWith("url: ") && !line.startsWith("req: "),
  );
  return {
    status: result.status,
    stdout: result.stdout,
    stderr,
    /** What the stub saw, e.g. `timeout 10000`, `tarball requested`. */
    stub: lines.filter((line) => line.startsWith("stub: ")).map((line) => line.slice("stub: ".length)),
    /** Every URL the launcher fetched, in order. */
    urls: lines.filter((line) => line.startsWith("url: ")).map((line) => line.slice("url: ".length)),
    /** The options of every request, in order. */
    requests: lines
      .filter((line) => line.startsWith("req: "))
      .map((line) => JSON.parse(line.slice("req: ".length)) as { url: string; headers: [string, string][]; options: string[] }),
    notices: stderr.filter((line) => line.startsWith(NOTICE)),
    failures: stderr.filter((line) => line.startsWith(FAILURE)),
  };
}

const latest = (version: string, extra: Record<string, string> = {}): Answer => ({
  type: "json",
  json: { version, dist: { tarball: `https://registry.npmjs.org/fake/-/fake-${version}.tgz`, ...extra } },
});

/** A gzipped ustar archive laid out like `npm pack` output (`package/…`). */
function npmPack(files: Record<string, string>): Buffer {
  const blocks: Buffer[] = [];
  for (const [name, text] of Object.entries(files)) {
    const body = Buffer.from(text);
    const header = Buffer.alloc(512);
    header.write(`package/${name}`, 0);
    header.write("0000644\0", 100);
    header.write("0000000\0", 108);
    header.write("0000000\0", 116);
    header.write(`${body.length.toString(8).padStart(11, "0")}\0`, 124);
    header.write("00000000000\0", 136);
    header.write("        ", 148);
    header.write("0", 156);
    header.write("ustar\0", 257);
    header.write("00", 263);
    const checksum = header.reduce((sum, byte) => sum + byte, 0);
    header.write(`${checksum.toString(8).padStart(6, "0")}\0 `, 148);
    blocks.push(header, body, Buffer.alloc((512 - (body.length % 512)) % 512));
  }
  blocks.push(Buffer.alloc(1024));
  return gzipSync(Buffer.concat(blocks));
}

/** `latest` pointing at a real, verifiable tarball of a fake CLI. */
function published(version: string) {
  const tgz = npmPack({
    "package.json": JSON.stringify({ name: "@vibe-cafe/vibe-usage", version, type: "module" }),
    "src/index.js": `export async function run(argv) { process.stdout.write("ran ${version} " + argv.join(" ") + "\\n"); }\n`,
  });
  return {
    latest: latest(version, { integrity: `sha512-${createHash("sha512").update(tgz).digest("base64")}` }),
    tarball: { type: "bytes", base64: tgz.toString("base64") } as Answer,
  };
}

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

    // A proxy login page answering 200: the JSON parser's own error, as before.
    const portal = launch({ latest: { type: "body", body: "<html>login</html>" } });
    expect(portal.status).toBe(1);
    expect(portal.notices).toEqual([]);
    expect(portal.stderr[0]).toMatch(new RegExp(`^${FAILURE} SyntaxError: `));
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
    expect(run.stub).not.toContain("tarball requested");
  });

  it("installs a first download with the long waits and runs it", () => {
    const run = launch(published("0.16.0"));
    expect(run.status).toBe(0);
    expect(run.stdout).toBe("ran 0.16.0 sync\n");
    expect(run.stderr).toEqual([]);
    // Nothing cached to fall back on: wait as long as before.
    expect(run.stub).toEqual(["timeout 20000", "timeout 120000", "tarball requested"]);
    const installed = join(home, "cli-cache", "0.16.0");
    expect(existsSync(join(installed, "complete"))).toBe(true);
    expect(existsSync(join(installed, "package", "src", "index.js"))).toBe(true);
    expect(readdirSync(join(home, "cli-cache")).filter((name) => name.startsWith(".staging-"))).toEqual([]);
  });

  it("waits less for latest and its download when a cached copy can stand in", () => {
    cache({ "0.14.11": {} });
    const run = launch(published("0.16.0"));
    expect(run.status).toBe(0);
    expect(run.stdout).toBe("ran 0.16.0 sync\n");
    expect(run.stub).toEqual(["timeout 10000", "timeout 15000", "tarball requested"]);
  });

  for (const [name, tarball, extra, stage] of [
    ["the download fails", { type: "status", status: 404 }, {}, "download"],
    ["the download drops", { type: "network" }, {}, "download"],
    ["the download does not verify", { type: "body", body: "not the package" }, { integrity: "sha512-AAAA" }, "download"],
    ["the package will not unpack", { type: "body", body: "not a gzip archive" }, {}, "install"],
  ] as const) {
    it(`falls back to the cache when a new latest is out and ${name}`, () => {
      cache({ "0.14.11": {} });
      const run = launch({ latest: latest("0.16.0", extra), tarball });
      expect(run.status).toBe(0);
      expect(run.stdout).toBe("ran 0.14.11 sync\n");
      expect(run.failures).toEqual([]);
      expect(run.notices).toHaveLength(1);
      expect(run.notices[0]).toMatch(
        new RegExp(`^${NOTICE} stage=${stage}; running cached @vibe-cafe/vibe-usage@0\\.14\\.11 `),
      );
      expect(readdirSync(join(home, "cli-cache")).filter((name) => name.startsWith(".staging-"))).toEqual([]);
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
  });

  // #34: once run() has started, a crash is the CLI's own failure. Without the
  // launcher prefix the app records it as a CLI exit, not a startup failure.
  it("prints a crash inside run() like Node would, without the launcher prefix", () => {
    cache({ "0.14.11": { crashes: true } });
    const run = launch({ latest: latest("0.14.11") });
    expect(run.status).toBe(1);
    expect(run.failures).toEqual([]);
    expect(run.stderr[0]).toBe("Error: cli crashed");
    expect(run.stderr.slice(1).every((line) => line.trimStart().startsWith("at "))).toBe(true);
  });

  it("still reports a CLI that fails to load as a launcher failure", () => {
    cache({ "0.14.11": { brokenImport: true } });
    const run = launch({ latest: latest("0.14.11") });
    expect(run.status).toBe(1);
    expect(run.stdout).toBe("");
    expect(run.failures[0]).toBe(`${FAILURE} Error: cli failed to load`);
  });

  it("does not retry a crashing fallback version", () => {
    cache({ "0.14.9": {}, "0.14.11": { crashes: true } });
    const run = launch({ latest: { type: "network" } });
    expect(run.status).toBe(1);
    expect(run.stdout).toBe("ran 0.14.11 sync\n");
    expect(run.notices).toHaveLength(1);
  });
});

describe("cli-bootstrap: the registry follows ~/.npmrc", () => {
  const OFFICIAL = "https://registry.npmjs.org/@vibe-cafe/vibe-usage/latest";
  const npmrc = (text: string) => writeFileSync(join(home, ".npmrc"), text);
  const metadataUrl = (plan: { latest: Answer; tarball?: Answer } = { latest: latest("0.14.11") }) => {
    cache({ "0.14.11": {} });
    const run = launch(plan);
    expect(run.status).toBe(0);
    return run.urls[0];
  };

  it("asks registry.npmjs.org without an npmrc", () => {
    expect(metadataUrl()).toBe(OFFICIAL);
  });

  it("asks the mirror the npmrc names and downloads the tarball it returns", () => {
    npmrc("registry=https://registry.npmmirror.com/\n");
    const mirror = published("0.16.0");
    const tarballUrl = "https://registry.npmmirror.com/@vibe-cafe/vibe-usage/-/vibe-usage-0.16.0.tgz";
    const dist = (mirror.latest as { json: { dist: Record<string, string> } }).json.dist;
    dist.tarball = tarballUrl;
    const run = launch(mirror);
    expect(run.status).toBe(0);
    expect(run.stdout).toBe("ran 0.16.0 sync\n");
    expect(run.urls).toEqual(["https://registry.npmmirror.com/@vibe-cafe/vibe-usage/latest", tarballUrl]);
  });

  it("reads the file the way npm does: comments, quotes, CRLF, BOM, last assignment wins", () => {
    npmrc(
      "\uFEFF; registry=https://commented.example\r\n" +
        "# registry=https://also-commented.example\r\n" +
        "registry=https://first.example\r\n" +
        'registry = "https://npm.example.com/repository/npm/"\r\n',
    );
    expect(metadataUrl()).toBe("https://npm.example.com/repository/npm/@vibe-cafe/vibe-usage/latest");
  });

  it("ends an unquoted value at an inline comment, as npm does", () => {
    for (const text of [
      "registry=https://registry.npmmirror.com/ # 国内镜像\n",
      "registry=https://registry.npmmirror.com/ ; mirror\n",
      "registry=https://registry.npmmirror.com/#mirror\n",
    ]) {
      npmrc(text);
      expect(metadataUrl(), text).toBe("https://registry.npmmirror.com/@vibe-cafe/vibe-usage/latest");
    }
  });

  it("keeps an escaped # or ; as part of the value", () => {
    npmrc("registry=https://npm.example.com/a\\;b/ ; comment\n");
    expect(metadataUrl()).toBe("https://npm.example.com/a;b/@vibe-cafe/vibe-usage/latest");
    // `\#` survives as `#`, which makes the address a URL fragment: not followed.
    npmrc("registry=https://npm.example.com/\\#frag\n");
    expect(metadataUrl()).toBe(OFFICIAL);
  });

  it("never sends credentials from the npmrc, whichever registry it asks", () => {
    const secrets = ["FAKE-mirror-token-123", "FAKE-npmjs-token-456", "ZmFrZTpmYWtl"];
    for (const [text, host] of [
      // A token for the mirror: the mirror is skipped, the official registry is asked.
      [`registry=https://registry.npmmirror.com/\n//registry.npmmirror.com/:_authToken=${secrets[0]}\n`, "registry.npmjs.org"],
      // A token for another host: the mirror is asked, without it.
      [`registry=https://registry.npmmirror.com/\n//registry.npmjs.org/:_authToken=${secrets[1]}\n//registry.npmjs.org/:_auth=${secrets[2]}\n`, "registry.npmmirror.com"],
    ] as const) {
      rmSync(join(home, "cli-cache"), { recursive: true, force: true });
      npmrc(text);
      const plan = published("0.16.0");
      (plan.latest as { json: { dist: Record<string, string> } }).json.dist.tarball =
        `https://${host}/@vibe-cafe/vibe-usage/-/vibe-usage-0.16.0.tgz`;
      const run = launch(plan);
      expect(run.status).toBe(0);
      // Both the metadata and the tarball request, each with no header at all.
      expect(run.requests.map((request) => new URL(request.url).host)).toEqual([host, host]);
      for (const request of run.requests) {
        expect(request.headers).toEqual([]);
        expect(request.options).toEqual(["signal"]);
      }
      const seen = [run.stdout, ...run.stderr, ...run.urls].join("\n");
      for (const secret of secrets) expect(seen).not.toContain(secret);
    }
  });

  it("prefers the @vibe-cafe scope's registry", () => {
    npmrc("@vibe-cafe:registry=https://scoped.example/npm\nregistry=https://registry.npmmirror.com\n");
    expect(metadataUrl()).toBe("https://scoped.example/npm/@vibe-cafe/vibe-usage/latest");
  });

  it("keeps a token for registry.npmjs.org from changing anything", () => {
    npmrc("//registry.npmjs.org/:_authToken=npm_abc\n");
    expect(metadataUrl()).toBe(OFFICIAL);
  });

  for (const [name, text] of [
    ["plain http", "registry=http://registry.npmmirror.com/\n"],
    ["a registry it holds a token for", "registry=https://npm.corp.example/\n//npm.corp.example/:_authToken=secret\n"],
    ["a registry it holds a password for", "registry=https://npm.corp.example:8443/r/\n//npm.corp.example:8443/r/:_password=c2VjcmV0\n"],
    ["an unscoped credential", "registry=https://npm.corp.example/\n_auth=dXNlcjpwYXNz\n"],
    ["credentials in the URL", "registry=https://user:pass@npm.corp.example/\n"],
    ["an unexpanded placeholder", "registry=${NPM_REGISTRY}\n"],
    ["something that is not a URL", "registry=registry.npmmirror.com\n"],
  ] as const) {
    it(`stays on registry.npmjs.org for ${name}`, () => {
      npmrc(text);
      expect(metadataUrl()).toBe(OFFICIAL);
    });
  }

  it("falls back to the cache when the mirror is unreachable, like the official registry", () => {
    npmrc("registry=https://registry.npmmirror.com\n");
    cache({ "0.14.11": {} });
    const run = launch({ latest: { type: "network" } });
    expect(run.status).toBe(0);
    expect(run.stdout).toBe("ran 0.14.11 sync\n");
    expect(run.urls).toEqual(["https://registry.npmmirror.com/@vibe-cafe/vibe-usage/latest"]);
    expect(run.notices).toHaveLength(1);
    expect(run.notices[0]).toMatch(new RegExp(`^${NOTICE} stage=network; `));
  });
});
