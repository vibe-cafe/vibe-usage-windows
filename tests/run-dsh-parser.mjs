#!/usr/bin/env node
/**
 * Subprocess entry point for the DSH vendored-parser regression tests.
 *
 * The vendored CLI is plain ESM with no build step, but it resolves its DSH root
 * from module-load environment state, so tests must not import it into the
 * vitest process. This driver is spawned with `--experimental-*`-free plain Node
 * and prints the parser's raw result as JSON on stdout.
 *
 *   node run-vendored-dsh-parser.mjs <sessions-dir>
 *
 * The parser path is derived from this file's own location, so the tests always
 * exercise the CLI snapshot that ships inside the installer.
 * `VIBE_USAGE_DSH_PARSER_PATH` overrides it; the guard test uses that to prove
 * the suite still fails against a pre-fix parser without touching the snapshot.
 */

import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const sessionsDir = process.argv[2];
if (!sessionsDir) {
  console.error('usage: node run-vendored-dsh-parser.mjs <sessions-dir>');
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const parserPath = process.env.VIBE_USAGE_DSH_PARSER_PATH?.trim()
  || join(here, '..', 'src-tauri', 'resources', 'cli', 'src', 'parsers', 'dsh.js');

process.env.VIBE_USAGE_DSH_SESSIONS = sessionsDir;

const out = { ok: false };
try {
  const mod = await import(pathToFileURL(parserPath).href);
  const result = await mod.parse();
  out.ok = true;
  out.buckets = result.buckets ?? [];
  out.sessions = result.sessions ?? [];
  out.skipped = result.skipped ?? false;
  out.warnings = result.warnings ?? [];
} catch (error) {
  out.error = {
    name: error?.name ?? 'Error',
    code: error?.code ?? null,
    message: error?.message ?? String(error),
  };
}

process.stdout.write(JSON.stringify(out) + '\n');
