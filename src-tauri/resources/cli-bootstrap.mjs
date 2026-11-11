#!/usr/bin/env node
/**
 * Runs the published `@vibe-cafe/vibe-usage` CLI with this app's bundled Node.
 *
 * The app used to ship a *vendored* copy of the CLI, patched with Windows fixes
 * so a release never depended on how long an upstream merge took. Those fixes
 * now live upstream (vibe-usage 0.14.1: the cmd.exe `start` login, backslash
 * cwd splitting, and the `%LOCALAPPDATA%` Amp/OpenCode roots), so the copy is
 * gone and this bootstrap replaces it.
 *
 * It always resolves the registry's `latest` dist-tag — never a pinned version.
 * A pin rots silently and freezes users out of every CLI fix, which is the
 * policy the macOS app already follows. The resolved version is cached under
 * the app's own directory, so a run costs one small metadata request and the
 * package is only downloaded when `latest` actually moves.
 *
 * When `latest` cannot be resolved or fetched — the registry is unreachable,
 * answers with an error, or the new package will not download or install — the
 * newest complete version already in the cache runs instead, with one notice
 * line on stderr (`NOTICE_PREFIX`). Users on a flaky route to npm would
 * otherwise lose a whole sync to every dropped connection. This is a fallback,
 * not a pin: every run still asks for `latest` first and moves on as soon as
 * the registry answers. With nothing cached the launcher fails as it always
 * has, with the same messages.
 *
 * `src/index.js` is imported in-process rather than spawned: the CLI's entry
 * point is `run(process.argv.slice(2))`, so stdout/stderr and the exit code are
 * exactly what the parent already parses.
 */
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { gunzipSync } from 'node:zlib';

const PACKAGE = '@vibe-cafe/vibe-usage';
const METADATA_URL = `https://registry.npmjs.org/${PACKAGE}/latest`;
const ENTRY = 'src/index.js';
/**
 * Prefix of the one line written when a cached version stands in for
 * `latest`. The app strips it before reading the CLI's own output and keeps
 * only the `stage=` word; it must never start with `vibe-usage launcher:`,
 * which means the CLI did not run at all.
 */
const NOTICE_PREFIX = 'vibe-usage launcher notice:';
/**
 * How long `latest` may take. The app ends a sync 120 s after starting this
 * process, so with a cached copy that can stand in the wait is cut short
 * enough to leave the CLI time to scan and upload: at most 10 + 15 s before
 * falling back, against a typical sync of about 20 s. The tarball is ~250 KB —
 * a few seconds on a working route; on a stalled one more waiting does not
 * help. With nothing cached there is no alternative, so the waits stay long.
 */
const METADATA_TIMEOUT_WITH_CACHE_MS = 10_000;
const METADATA_TIMEOUT_MS = 20_000;
const DOWNLOAD_TIMEOUT_WITH_CACHE_MS = 15_000;
const DOWNLOAD_TIMEOUT_MS = 120_000;

/** Where the app keeps its own files; falls back to `~/.vibe-usage`. */
function cacheRoot() {
  const configured = process.env.VIBE_USAGE_CONFIG_DIR?.trim();
  return join(configured || join(homedir(), '.vibe-usage'), 'cli-cache');
}

function fail(message) {
  process.stderr.write(`vibe-usage launcher: ${message}\n`);
  process.exit(1);
}

/**
 * A failure the launcher can name. `stage` is one of `network`, `registry`,
 * `download`, `install` (the same words the app's sync history uses); the
 * message is what `fail()` prints when no cached version can stand in, and the
 * app's sync history classifies those messages by their fixed beginnings.
 * Errors from Node itself keep their own text and get their stage from the
 * step that threw them (`resolvePackageDir`).
 */
class LauncherError extends Error {
  constructor(stage, message) {
    super(message);
    this.stage = stage;
  }
}

/** Anything thrown here without a `stage` is a network failure. */
async function fetchLatest(timeoutMs) {
  const res = await fetch(METADATA_URL, { signal: AbortSignal.timeout(timeoutMs) });
  if (!res.ok) throw new LauncherError('registry', `registry returned ${res.status} for ${PACKAGE}@latest`);
  let manifest;
  try {
    manifest = await res.json();
  } catch (err) {
    // A captive portal or proxy page can answer 200 with HTML: a registry
    // problem for the fallback, while with nothing cached the original error
    // is still what gets printed. A dropped connection fails the body read
    // itself and stays a network failure.
    throw err?.name === 'SyntaxError' ? Object.assign(err, { stage: 'registry' }) : err;
  }
  if (typeof manifest?.version !== 'string' || typeof manifest?.dist?.tarball !== 'string') {
    throw new LauncherError('registry', `registry answered without a version/tarball for ${PACKAGE}@latest`);
  }
  return { version: manifest.version, tarball: manifest.dist.tarball, integrity: manifest.dist.integrity };
}

/** `<cache>/<version>/package` when that version is fully installed. */
function cachedPackage(version) {
  const dir = join(cacheRoot(), version);
  return existsSync(join(dir, 'complete')) && existsSync(join(dir, 'package', ENTRY))
    ? join(dir, 'package')
    : null;
}

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

/** Highest first; a release beats a prerelease of the same number. */
function compareVersions(a, b) {
  const [, ...pa] = SEMVER.exec(a);
  const [, ...pb] = SEMVER.exec(b);
  for (let i = 0; i < 3; i++) {
    const diff = Number(pb[i]) - Number(pa[i]);
    if (diff !== 0) return diff;
  }
  if (!pa[3] !== !pb[3]) return pa[3] ? 1 : -1;
  return (pb[3] ?? '').localeCompare(pa[3] ?? '');
}

/**
 * The newest version that a previous run installed completely: a version-named
 * directory with the `complete` marker and the entry file. Staging directories
 * and anything half-written are skipped.
 */
function newestCached() {
  let names;
  try {
    names = readdirSync(cacheRoot());
  } catch {
    return null;
  }
  const version = names
    .filter(name => SEMVER.test(name))
    .sort(compareVersions)
    .find(name => cachedPackage(name));
  return version ? { version, packageDir: cachedPackage(version) } : null;
}

/** One line, no stack: the notice is for people reading the sync log. */
function shortReason(err) {
  const message = String(err?.message || err).split('\n')[0];
  const code = err?.cause?.code;
  return (code && !message.includes(code) ? `${message}: ${code}` : message).slice(0, 200);
}

/**
 * `latest` is out of reach at `stage`: run the newest cached version, or fail
 * with the same text as before when there is none.
 */
function fallBackToCache(stage, err) {
  const cached = newestCached();
  if (!cached) fail(err instanceof LauncherError ? err.message : err?.stack || String(err));
  process.stderr.write(
    `${NOTICE_PREFIX} stage=${stage}; running cached ${PACKAGE}@${cached.version} because latest is unavailable (${shortReason(err)})\n`,
  );
  return cached.packageDir;
}

/** Minimal ustar reader — enough for an `npm pack` tarball, no dependency. */
function extractTar(gz, destination) {
  const buffer = gunzipSync(gz);
  let offset = 0;
  let longName = null;
  while (offset + 512 <= buffer.length) {
    const header = buffer.subarray(offset, offset + 512);
    // Two zero blocks end the archive; anything else here is padding.
    if (header.every(byte => byte === 0)) break;
    const read = (start, length) => header.subarray(start, start + length).toString('utf8').replace(/\0.*$/, '');
    const size = parseInt(read(124, 12).trim() || '0', 8) || 0;
    const type = read(156, 1);
    const prefix = read(345, 155);
    const name = longName ?? (prefix ? `${prefix}/${read(0, 100)}` : read(0, 100));
    longName = null;
    const body = buffer.subarray(offset + 512, offset + 512 + size);
    offset += 512 + Math.ceil(size / 512) * 512;

    if (type === 'L') { // GNU long name: the body names the next entry.
      longName = body.toString('utf8').replace(/\0.*$/, '');
      continue;
    }
    if (type === 'x' || type === 'g' || type === 'K') continue; // pax metadata
    if (!name || name.includes('..') || name.startsWith('/')) continue;
    const target = join(destination, name);
    if (type === '5' || name.endsWith('/')) {
      mkdirSync(target, { recursive: true });
    } else if (type === '0' || type === '\0' || type === '') {
      mkdirSync(dirname(target), { recursive: true });
      writeFileSync(target, body);
    }
  }
}

/** The verified tarball bytes; any failure here is the `download` stage. */
async function downloadTarball(version, tarball, integrity, timeoutMs) {
  const res = await fetch(tarball, { signal: AbortSignal.timeout(timeoutMs) });
  if (!res.ok) throw new LauncherError('download', `download failed (${res.status}) for ${PACKAGE}@${version}`);
  const bytes = Buffer.from(await res.arrayBuffer());

  // The registry states sha512; verify it rather than executing whatever came
  // back.
  if (typeof integrity === 'string' && integrity.startsWith('sha512-')) {
    const digest = createHash('sha512').update(bytes).digest('base64');
    if (digest !== integrity.slice('sha512-'.length)) {
      throw new LauncherError('download', `integrity mismatch for ${PACKAGE}@${version}`);
    }
  }
  return bytes;
}

/** Unpacks into the cache; any failure here is the `install` stage. */
function install(version, bytes) {
  const target = join(cacheRoot(), version);
  // A sync from the scheduler, a manual sync and a quota fetch can all start at
  // once, so the tree is built in a private staging directory and moved into
  // place with a single atomic rename. Extracting straight into `target` would
  // let one process delete the version directory while another is writing it.
  const staging = join(
    cacheRoot(),
    `.staging-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2)}`,
  );
  mkdirSync(cacheRoot(), { recursive: true });
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  try {
    // `npm pack` names every entry `package/…`, so the archive itself provides
    // that level — extracting into an extra `package/` would nest it twice.
    extractTar(bytes, staging);
    if (!existsSync(join(staging, 'package', ENTRY))) {
      throw new LauncherError('install', `${PACKAGE}@${version} did not contain ${ENTRY}`);
    }
    // Written before the rename, so a version directory that exists is complete.
    writeFileSync(join(staging, 'complete'), version);
    try {
      renameSync(staging, target);
    } catch (err) {
      // Losing the race is not an error: the winner's copy is byte-identical
      // (same tarball, same verified digest) and already complete.
      if (!existsSync(join(target, 'complete'))) {
        throw new LauncherError('install', `could not install ${PACKAGE}@${version}: ${err?.message || err}`);
      }
    }
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  return join(target, 'package');
}

/**
 * The package directory to run. Only resolving and fetching `latest` may fall
 * back to the cache — never the CLI run itself, which must happen once.
 */
async function resolvePackageDir() {
  const hasCache = newestCached() !== null;
  let latest;
  try {
    latest = await fetchLatest(hasCache ? METADATA_TIMEOUT_WITH_CACHE_MS : METADATA_TIMEOUT_MS);
  } catch (err) {
    return fallBackToCache(err?.stage ?? 'network', err);
  }
  const { version, tarball, integrity } = latest;
  const cached = cachedPackage(version);
  if (cached) return cached;
  let bytes;
  try {
    bytes = await downloadTarball(
      version,
      tarball,
      integrity,
      hasCache ? DOWNLOAD_TIMEOUT_WITH_CACHE_MS : DOWNLOAD_TIMEOUT_MS,
    );
  } catch (err) {
    return fallBackToCache(err?.stage ?? 'download', err);
  }
  try {
    return install(version, bytes);
  } catch (err) {
    return fallBackToCache(err?.stage ?? 'install', err);
  }
}

async function main() {
  const packageDir = await resolvePackageDir();
  const { run } = await import(pathToFileURL(join(packageDir, ENTRY)).href);
  await run(process.argv.slice(2));
}

main().catch(err => fail(err?.stack || String(err)));
