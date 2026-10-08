/**
 * Resolves the CLI the app actually runs — `@vibe-cafe/vibe-usage@latest` — the
 * same way the shipped launcher does, and hands its paths to the tests.
 *
 * The app no longer bundles a CLI snapshot: `src-tauri/resources/cli-bootstrap.mjs`
 * resolves npm's `latest` dist-tag at run time. Tests that used to read
 * `src-tauri/resources/cli/...` therefore have to ask for the package instead of
 * assuming a path. Pointing them at the *published* package is the point: these
 * assertions cover Windows behaviour the app depends on (extra roots, config and
 * state dirs, quota contract, client identity), and a vendored copy would hide
 * exactly the drift that made 0.5.12 ship a stale DSH parser.
 *
 * Resolution goes through the real launcher, so the test exercises the shipped
 * code path, and the launcher's per-version cache keeps it to one metadata
 * request after the first fetch. `VIBE_USAGE_CLI_DIR` overrides the result, which
 * is how a run can be pinned to a local checkout when diagnosing upstream.
 */
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LAUNCHER = join(ROOT, "src-tauri", "resources", "cli-bootstrap.mjs");
/** Stable across runs so the launcher's own cache makes repeat runs cheap.
 *  Deliberately inside the repo (not node_modules): Vite externalizes
 *  node_modules, and `vi.resetModules()` must keep working for these imports. */
const CACHE = join(ROOT, ".cli-under-test");

let cached;

/** Absolute path of the CLI package root (`…/package`) under test. */
export function cliPackageRoot() {
  if (cached) return cached;

  const override = process.env.VIBE_USAGE_CLI_DIR?.trim();
  if (override) {
    const dir = resolve(override);
    if (!existsSync(join(dir, "package.json"))) {
      throw new Error(`VIBE_USAGE_CLI_DIR=${override} has no package.json`);
    }
    cached = dir;
    return cached;
  }

  execFileSync(process.execPath, [LAUNCHER, "--version"], {
    cwd: ROOT,
    stdio: "pipe",
    timeout: 180_000,
    env: { ...process.env, VIBE_USAGE_CONFIG_DIR: CACHE, VIBE_USAGE_DEV: "0" },
  });

  const versions = readdirSync(join(CACHE, "cli-cache"), { withFileTypes: true })
    .filter(entry =>
      entry.isDirectory()
      && !entry.name.startsWith(".staging-") // a concurrent install in flight
      && existsSync(join(CACHE, "cli-cache", entry.name, "complete")))
    .map(entry => entry.name);
  if (versions.length !== 1) {
    throw new Error(`expected exactly one cached CLI under ${CACHE}/cli-cache, found ${versions.join(", ") || "none"}`);
  }
  cached = join(CACHE, "cli-cache", versions[0], "package");
  return cached;
}

/** Absolute path of a file inside the CLI package. */
export function cliPath(relative) {
  return join(cliPackageRoot(), relative);
}

/** The version the launcher resolved. */
export function cliVersion() {
  return JSON.parse(
    readFileSync(join(cliPackageRoot(), "package.json"), "utf8"),
  ).version;
}

/** Env for spawning the CLI entry point out of process. */
export function cliEnv(extra = {}) {
  return { ...process.env, VIBE_USAGE_DEV: "0", ...extra };
}
