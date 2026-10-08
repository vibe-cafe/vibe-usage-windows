import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { cliPath } from "./cliUnderTest.mjs";

/**
 * Behavioural coverage for the Windows OpenCode root resolution the app relies on.
 *
 * Windows builds keep OpenCode session data under `%LOCALAPPDATA%\opencode`, and
 * the CLI carries that root in its default list (upstream since 0.14.1). A source
 * fingerprint cannot show that the root is *reachable*: it is a control-flow
 * change inside `defaultOpenCodeRoots()`, and the failure mode this suite exists
 * for is a change that leaves the function syntactically fine while producing the
 * wrong roots.
 *
 * The tests therefore call the exported `getOpenCodeStores()` — the public
 * root-discovery entry point the parser itself uses — from the published package
 * the launcher resolved, in a subprocess whose `USERPROFILE` and `LOCALAPPDATA`
 * point at throwaway fixture directories. The developer's real profile is never
 * consulted, and `VIBE_USAGE_OPENCODE_DIRS` is cleared so the environment cannot
 * silently replace the defaults under test.
 */

const HERE = dirname(fileURLToPath(import.meta.url));
const MODULE_PATH = cliPath("src/opencode-roots.js");
const PROBE = join(HERE, "run-opencode-roots-probe.mjs");

const tempDirs: string[] = [];

afterEach(() => {
  while (tempDirs.length > 0) rmSync(tempDirs.pop()!, { recursive: true, force: true });
});

interface ProbeResult {
  ok: boolean;
  platform?: string;
  home?: string;
  localAppData?: string | null;
  stores?: Array<{ kind: string; path: string }>;
  warnings?: string[];
  error?: { name: string; message: string };
}

interface Fixture {
  /** Canonical sqlite store under the XDG root, when it was created. */
  xdg: string | null;
  /** Canonical sqlite store under %LOCALAPPDATA%\opencode, when it was created. */
  windows: string | null;
  home: string;
  localAppData: string;
}

/** Build a throwaway profile with real (empty) OpenCode sqlite stores. */
function makeFixture(options: { xdg: boolean; windows: boolean }): Fixture {
  const root = mkdtempSync(join(tmpdir(), "vibe-usage-opencode-"));
  tempDirs.push(root);
  const home = join(root, "home");
  const localAppData = join(root, "localappdata");

  const create = (dbPath: string) => {
    mkdirSync(dirname(dbPath), { recursive: true });
    writeFileSync(dbPath, "");
    return realpathSync(dbPath);
  };

  return {
    xdg: options.xdg ? create(join(home, ".local", "share", "opencode", "opencode.db")) : null,
    windows: options.windows ? create(join(localAppData, "opencode", "opencode.db")) : null,
    home,
    localAppData,
  };
}

function probe(
  fixture: Fixture,
  options: { localAppData?: string | null; platform?: string; modulePath?: string } = {},
): ProbeResult {
  // USERPROFILE is what `homedir()` reads on Windows; HOME is its POSIX
  // equivalent. Setting both keeps the fixture isolated on every platform, so
  // this suite can be run (and trusted) on a macOS contributor machine too —
  // with only USERPROFILE set, a POSIX run used the developer's real profile and
  // every assertion below failed for the wrong reason.
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    USERPROFILE: fixture.home,
    HOME: fixture.home,
  };
  if (options.localAppData === null) delete env.LOCALAPPDATA;
  else env.LOCALAPPDATA = options.localAppData ?? fixture.localAppData;
  // The override would bypass the defaults entirely, which is the opposite of
  // what these tests assert.
  delete env.VIBE_USAGE_OPENCODE_DIRS;
  env.VIBE_USAGE_OPENCODE_ROOTS_PATH = options.modulePath ?? MODULE_PATH;

  const proc = spawnSync(process.execPath, [PROBE, options.platform ?? "-"], { encoding: "utf8", env });
  if (proc.status !== 0 || !proc.stdout) {
    throw new Error(`probe exited ${proc.status}: ${proc.stderr || proc.stdout}`);
  }
  const result = JSON.parse(proc.stdout) as ProbeResult;
  if (!result.ok) throw new Error(`probe failed: ${result.error?.message ?? "unknown"}`);
  return result;
}

const paths = (result: ProbeResult) => (result.stores ?? []).map((store) => store.path);

describe("vendored OpenCode roots — Windows patch behaviour", () => {
  it("keeps the XDG root and adds %LOCALAPPDATA%\\opencode, XDG first", () => {
    const fixture = makeFixture({ xdg: true, windows: true });

    // The Windows branch is keyed off `process.platform`, so every case that
    // asserts it states the platform explicitly — otherwise this suite would
    // silently assert Linux/macOS behaviour on those hosts and only mean
    // something on a Windows machine.
    const result = probe(fixture, { platform: "win32" });

    // Both roots are reachable, and the added one does not displace or precede
    // the upstream default.
    expect(paths(result)).toEqual([fixture.xdg, fixture.windows]);
    expect(result.stores?.map((store) => store.kind)).toEqual(["sqlite", "sqlite"]);
    expect(result.warnings).toEqual([]);
  });

  it("keeps the XDG root when no Windows store exists", () => {
    const fixture = makeFixture({ xdg: true, windows: false });

    const result = probe(fixture, { platform: "win32" });

    // An absent root is skipped silently rather than warned about: it is a
    // default candidate, not a user-configured directory.
    expect(paths(result)).toEqual([fixture.xdg]);
    expect(result.warnings).toEqual([]);
  });

  it("finds %LOCALAPPDATA%\\opencode when the XDG root holds no store", () => {
    const fixture = makeFixture({ xdg: false, windows: true });

    const result = probe(fixture, { platform: "win32" });

    expect(paths(result)).toEqual([fixture.windows]);
  });

  it("falls back to the XDG root alone when LOCALAPPDATA is not set", () => {
    const fixture = makeFixture({ xdg: true, windows: true });

    const result = probe(fixture, { localAppData: null, platform: "win32" });

    expect(result.localAppData).toBeNull();
    expect(paths(result)).toEqual([fixture.xdg]);
  });

  it("does not add the Windows root on non-Windows platforms", () => {
    const fixture = makeFixture({ xdg: true, windows: true });

    const result = probe(fixture, { platform: "linux" });

    // The probe override has to be observable, otherwise this case could pass
    // because the override never took effect.
    expect(result.platform).toBe("linux");
    expect(result.localAppData).toBe(fixture.localAppData);
    expect(paths(result)).toEqual([fixture.xdg]);
  });

  it("fails against a module without the Windows branch", () => {
    // Guard against a vacuous suite: strip the added branch from a copy of the
    // shipped module and confirm the assertions above would notice.
    const source = readFileSync(MODULE_PATH, "utf8").replace(/\r\n/g, "\n");
    const branch = `  const localAppData = process.env.LOCALAPPDATA?.trim();
  if (process.platform === 'win32' && localAppData) {
    return [xdg, join(localAppData, 'opencode')];
  }
`;
    expect(source, "the Windows branch must be present to remove it").toContain(branch);
    const unpatched = source.replace(branch, "");
    expect(unpatched).not.toBe(source);

    const dir = mkdtempSync(join(tmpdir(), "vibe-usage-opencode-unpatched-"));
    tempDirs.push(dir);
    const unpatchedPath = join(dir, "opencode-roots.js");
    writeFileSync(unpatchedPath, unpatched);

    const fixture = makeFixture({ xdg: false, windows: true });

    expect(paths(probe(fixture, { modulePath: unpatchedPath }))).toEqual([]);
  });
});
