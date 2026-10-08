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
 * `src/index.js` is imported in-process rather than spawned: the CLI's entry
 * point is `run(process.argv.slice(2))`, so stdout/stderr and the exit code are
 * exactly what the parent already parses.
 */
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { gunzipSync } from 'node:zlib';

const PACKAGE = '@vibe-cafe/vibe-usage';
const METADATA_URL = `https://registry.npmjs.org/${PACKAGE}/latest`;
const ENTRY = 'src/index.js';

/** Where the app keeps its own files; falls back to `~/.vibe-usage`. */
function cacheRoot() {
  const configured = process.env.VIBE_USAGE_CONFIG_DIR?.trim();
  return join(configured || join(homedir(), '.vibe-usage'), 'cli-cache');
}

function fail(message) {
  process.stderr.write(`vibe-usage launcher: ${message}\n`);
  process.exit(1);
}

async function fetchLatest() {
  const res = await fetch(METADATA_URL, { signal: AbortSignal.timeout(20_000) });
  if (!res.ok) fail(`registry returned ${res.status} for ${PACKAGE}@latest`);
  const manifest = await res.json();
  if (typeof manifest?.version !== 'string' || typeof manifest?.dist?.tarball !== 'string') {
    fail(`registry answered without a version/tarball for ${PACKAGE}@latest`);
  }
  return { version: manifest.version, tarball: manifest.dist.tarball, integrity: manifest.dist.integrity };
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

async function download(version, tarball, integrity) {
  const res = await fetch(tarball, { signal: AbortSignal.timeout(120_000) });
  if (!res.ok) fail(`download failed (${res.status}) for ${PACKAGE}@${version}`);
  const bytes = Buffer.from(await res.arrayBuffer());

  // The registry states sha512; verify it rather than executing whatever came
  // back.
  if (typeof integrity === 'string' && integrity.startsWith('sha512-')) {
    const digest = createHash('sha512').update(bytes).digest('base64');
    if (digest !== integrity.slice('sha512-'.length)) {
      fail(`integrity mismatch for ${PACKAGE}@${version}`);
    }
  }

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
      fail(`${PACKAGE}@${version} did not contain ${ENTRY}`);
    }
    // Written before the rename, so a version directory that exists is complete.
    writeFileSync(join(staging, 'complete'), version);
    try {
      renameSync(staging, target);
    } catch (err) {
      // Losing the race is not an error: the winner's copy is byte-identical
      // (same tarball, same verified digest) and already complete.
      if (!existsSync(join(target, 'complete'))) {
        fail(`could not install ${PACKAGE}@${version}: ${err?.message || err}`);
      }
    }
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  return join(target, 'package');
}

async function main() {
  const { version, tarball, integrity } = await fetchLatest();
  const cached = join(cacheRoot(), version);
  const packageDir = existsSync(join(cached, 'complete')) && existsSync(join(cached, 'package', ENTRY))
    ? join(cached, 'package')
    : await download(version, tarball, integrity);

  const { run } = await import(pathToFileURL(join(packageDir, ENTRY)).href);
  await run(process.argv.slice(2));
}

main().catch(err => fail(err?.stack || String(err)));
