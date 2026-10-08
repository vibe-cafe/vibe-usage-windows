#!/usr/bin/env node
// Vendor the @vibe-cafe/vibe-usage CLI into src-tauri/resources/cli and apply
// the Windows patches (upstreamed via the windows-support PR; vendored copies
// stay patched so releases don't depend on upstream merge timing).
//
// Usage:
//   node scripts/vendor-cli.mjs                  # npm pack @vibe-cafe/vibe-usage@latest
//   node scripts/vendor-cli.mjs --from-local ../vibe-usage   # copy a local checkout
//
// The CLI has zero npm dependencies, so vendoring bin/ + src/ + package.json
// is sufficient — no node_modules.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const appPackage = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
const CLI_CHANNEL = appPackage.vibeUsageCliChannel;
if (CLI_CHANNEL !== "latest") {
  throw new Error("package.json vibeUsageCliChannel must be latest");
}
const destDir = path.join(root, "src-tauri", "resources", "cli");

function log(msg) {
  console.log(`[vendor-cli] ${msg}`);
}

function copyDir(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, entry.name);
    const d = path.join(dest, entry.name);
    if (entry.isDirectory()) copyDir(s, d);
    else fs.copyFileSync(s, d);
  }
}

// A dev checkout only names a reproducible source when it is clean: a dirty
// tree has no commit that describes the bytes being vendored, so record none.
function localCommit(abs) {
  try {
    const dirty = execFileSync("git", ["-C", abs, "status", "--porcelain"], { encoding: "utf8" }).trim();
    if (dirty) return null;
    return execFileSync("git", ["-C", abs, "rev-parse", "--short=12", "HEAD"], { encoding: "utf8" }).trim() || null;
  } catch {
    return null;
  }
}

function vendorFromLocal(localPath) {
  const abs = path.resolve(root, localPath);
  log(`vendoring from local checkout: ${abs}`);
  for (const p of ["bin", "src", "package.json"]) {
    if (!fs.existsSync(path.join(abs, p))) {
      throw new Error(`local checkout missing ${p}/ — wrong path?`);
    }
  }
  fs.rmSync(destDir, { recursive: true, force: true });
  copyDir(path.join(abs, "bin"), path.join(destDir, "bin"));
  copyDir(path.join(abs, "src"), path.join(destDir, "src"));
  fs.copyFileSync(path.join(abs, "package.json"), path.join(destDir, "package.json"));
  return { source: "local", commit: localCommit(abs) };
}

function vendorFromNpm() {
  log(`npm pack @vibe-cafe/vibe-usage@${CLI_CHANNEL}`);
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "vibe-cli-"));
  try {
    const out = execFileSync(
      "npm",
      ["pack", `@vibe-cafe/vibe-usage@${CLI_CHANNEL}`, "--pack-destination", tmp],
      { encoding: "utf8", shell: process.platform === "win32" },
    ).trim();
    const tarball = path.join(tmp, out.split("\n").pop().trim());
    execFileSync("tar", ["-xzf", tarball, "-C", tmp], { stdio: "inherit" });
    const pkgDir = path.join(tmp, "package");
    fs.rmSync(destDir, { recursive: true, force: true });
    copyDir(path.join(pkgDir, "bin"), path.join(destDir, "bin"));
    copyDir(path.join(pkgDir, "src"), path.join(destDir, "src"));
    fs.copyFileSync(path.join(pkgDir, "package.json"), path.join(destDir, "package.json"));
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  // A registry release has no reviewed commit to point at; the pin in
  // package.json#vibeUsageCliVersion is the identity instead.
  return { source: "npm", commit: null };
}

// ---------------------------------------------------------------------------
// Windows patches. Each patch aborts loudly when its anchor is missing so a
// CLI upgrade can't silently ship unpatched.

function patchFile(rel, replacements) {
  const file = path.join(destDir, rel);
  let content = fs.readFileSync(file, "utf8").replace(/\r\n/g, "\n");
  for (const [from, to, name] of replacements) {
    if (!content.includes(from)) {
      throw new Error(`patch anchor missing in ${rel} (${name}) — CLI changed; re-verify patches`);
    }
    content = content.split(from).join(to);
  }
  fs.writeFileSync(file, content);
  log(`patched ${rel}`);
}

function applyWindowsPatches() {
  // 1. `start` is a cmd.exe builtin — execFile('start', ...) fails on Windows.
  patchFile("src/init.js", [
    [
      `function openBrowser(url) {
  const cmds = { darwin: 'open', linux: 'xdg-open', win32: 'start' };
  const cmd = cmds[platform()] || cmds.linux;
  // Use execFile with args array to avoid shell injection via VIBE_USAGE_API_URL
  execFile(cmd, [url], () => {});
}`,
      `function openBrowser(url) {
  if (platform() === 'win32') {
    // \`start\` is a cmd.exe builtin, not an executable — go through cmd /c.
    // Empty title arg keeps quoted URLs intact; ^& escapes query ampersands.
    execFile('cmd', ['/c', 'start', '', url.replace(/&/g, '^&')], { windowsHide: true }, () => {});
    return;
  }
  const cmds = { darwin: 'open', linux: 'xdg-open' };
  const cmd = cmds[platform()] || cmds.linux;
  // Use execFile with args array to avoid shell injection via VIBE_USAGE_API_URL
  execFile(cmd, [url], () => {});
}`,
      "openBrowser win32",
    ],
  ]);

  // 2. Windows cwd uses backslashes — project extraction must split on both.
  patchFile("src/parsers/codex.js", [
    [
      "if (meta.cwd) return meta.cwd.split('/').pop() || 'unknown';",
      "if (meta.cwd) return meta.cwd.split(/[\\\\/]/).pop() || 'unknown';",
      "codex extractProject backslash",
    ],
  ]);
  patchFile("src/parsers/qwen-code.js", [
    [
      "const parts = cwd.split('/').filter(Boolean);",
      "const parts = cwd.split(/[\\\\/]/).filter(Boolean);",
      "qwen extractProject backslash",
    ],
  ]);

  // 3. OpenCode on Windows keeps session data under %LOCALAPPDATA%\opencode in
  // some builds, while upstream only ever probes the XDG location. Upstream
  // moved root resolution out of the parser into src/opencode-roots.js
  // (v0.10.31, extra-root support), so the probe is added to the default-root
  // list there. XDG stays first: this adds a root, it never replaces one.
  patchFile("src/opencode-roots.js", [
    [
      "import { accessSync, constants, realpathSync, statSync } from 'node:fs';",
      "import { accessSync, constants, existsSync, realpathSync, statSync } from 'node:fs';",
      "opencode roots fs helpers",
    ],
    [
      `  const defaults = override ? override.split(delimiter).map(p => p.trim()).filter(Boolean)
    : [join(homedir(), '.local', 'share', 'opencode')];`,
      `  const defaults = override ? override.split(delimiter).map(p => p.trim()).filter(Boolean)
    : defaultOpenCodeRoots();`,
      "opencode roots defaults",
    ],
    [
      "export function getOpenCodeStores({ extraRoots = [], onWarning = () => {} } = {}) {",
      `// XDG first, then the Windows per-user store. A root that does not resolve is
// skipped silently by the caller, so probing costs nothing when it is absent.
function defaultOpenCodeRoots() {
  const xdg = join(homedir(), '.local', 'share', 'opencode');
  const localAppData = process.env.LOCALAPPDATA?.trim();
  if (process.platform === 'win32' && localAppData) {
    return [xdg, join(localAppData, 'opencode')];
  }
  return [xdg];
}

export function getOpenCodeStores({ extraRoots = [], onWarning = () => {} } = {}) {`,
      "opencode windows data dir",
    ],
  ]);

  // 4. Amp on Windows: %LOCALAPPDATA%\\amp\\threads (XDG default kept last).
  patchFile("src/parsers/amp.js", [
    [
      "  if (process.env.XDG_DATA_HOME) return join(process.env.XDG_DATA_HOME, 'amp', 'threads');\n  return join(homedir(), '.local', 'share', 'amp', 'threads');",
      `  if (process.env.XDG_DATA_HOME) return join(process.env.XDG_DATA_HOME, 'amp', 'threads');
  if (process.platform === 'win32' && process.env.LOCALAPPDATA) {
    const winDir = join(process.env.LOCALAPPDATA, 'amp', 'threads');
    if (existsSync(winDir)) return winDir;
  }
  return join(homedir(), '.local', 'share', 'amp', 'threads');`,
      "amp windows data dir",
    ],
  ]);

  // 5. The app invokes the CLI from a bundled runtime. Keep CLI config/state
  // in the app config dir, and repair accidental directory-at-file-path cases
  // so sync cannot fail with EISDIR when writing config.json/state.json.
  patchFile("src/config.js", [
    [
      "import { readFileSync, writeFileSync, chmodSync, mkdirSync, existsSync } from 'node:fs';",
      "import { readFileSync, writeFileSync, chmodSync, mkdirSync, existsSync, renameSync, statSync } from 'node:fs';",
      "config fs helpers",
    ],
    [
      "export function getConfigPath() {",
      `function backupPath(path) {
  return \`\${path}.directory-backup-\${Date.now()}\`;
}

function moveDirectoryOutOfFilePath(path) {
  try {
    if (statSync(path).isDirectory()) {
      renameSync(path, backupPath(path));
    }
  } catch (err) {
    if (err?.code !== 'ENOENT') throw err;
  }
}

export function getConfigPath() {`,
      "config EISDIR repair helpers",
    ],
    [
      "  mkdirSync(CONFIG_DIR, { recursive: true });",
      "  mkdirSync(CONFIG_DIR, { recursive: true });\n  moveDirectoryOutOfFilePath(CONFIG_FILE);",
      "config save EISDIR repair",
    ],
  ]);

  patchFile("src/state.js", [
    [
      "import { readFileSync, writeFileSync, unlinkSync, mkdirSync, existsSync, renameSync, rmSync } from 'node:fs';",
      "import { readFileSync, writeFileSync, unlinkSync, mkdirSync, existsSync, renameSync, rmSync, statSync } from 'node:fs';",
      "state fs helpers",
    ],
    [
      "const STATE_DIR = process.env.VIBE_USAGE_STATE_DIR?.trim() || join(homedir(), '.vibe-usage');",
      "const STATE_DIR = process.env.VIBE_USAGE_STATE_DIR?.trim() || process.env.VIBE_USAGE_CONFIG_DIR?.trim() || join(homedir(), '.vibe-usage');",
      "state app dir override",
    ],
    [
      "export function getStatePath() {",
      `function backupPath(path) {
  return \`\${path}.directory-backup-\${Date.now()}\`;
}

function moveDirectoryOutOfFilePath(path) {
  try {
    if (statSync(path).isDirectory()) {
      renameSync(path, backupPath(path));
    }
  } catch (err) {
    if (err?.code !== 'ENOENT') throw err;
  }
}

export function getStatePath() {`,
      "state EISDIR repair helpers",
    ],
    [
      // Anchor on the mkdir line alone: the comment that used to follow it is
      // reworded whenever state.js gains a field (v0.11.1's `identity` did
      // exactly that), and an anchor that includes prose breaks for no reason.
      "  mkdirSync(STATE_DIR, { recursive: true });",
      "  mkdirSync(STATE_DIR, { recursive: true });\n  moveDirectoryOutOfFilePath(STATE_FILE);",
      "state save EISDIR repair",
    ],
  ]);
}

// ---------------------------------------------------------------------------

const localFlag = process.argv.indexOf("--from-local");
let sourceMetadata;
if (localFlag >= 0) {
  sourceMetadata = vendorFromLocal(process.argv[localFlag + 1] ?? "../vibe-usage");
} else {
  // A release must contain the registry's current dist-tag. Never silently
  // fall back to a sibling checkout; --from-local is an explicit dev-only path.
  sourceMetadata = vendorFromNpm();
}


// Preserve durable Codex accounting records on Windows, including cold re-vendors.
function applyCodexUsageRecordPatch() {
  fs.writeFileSync(path.join(destDir, "src/parsers/codex-usage-record.js"), `// Durable per-request usage can survive even when the UI token_count is missing.
// Convert both indexing and accounting passes identically; keep the source marker
// so the following UI mirror is not counted again when its cumulative total lags.
export function normalizeUsageRecord(obj) {
  if (obj?.type !== 'token_usage_record') return obj;
  const p = obj.payload;
  const usage = p?.usage;
  const total = p?.thread_token_usage;
  if (!usage || !total || !Number.isFinite(total.total_tokens) || total.total_tokens <= 0) return obj;
  for (const value of [usage.input_tokens, usage.output_tokens, usage.cached_input_tokens ?? 0, usage.reasoning_output_tokens ?? 0]) {
    if (!Number.isFinite(value) || value < 0) return obj;
  }
  return { ...obj, type: 'event_msg', payload: {
    type: 'token_count', usage_record: true,
    info: { last_token_usage: pickUsage(usage), total_token_usage: pickUsage(total) },
  }};
}

function pickUsage(value) {
  const keys = ['input_tokens', 'output_tokens', 'cached_input_tokens', 'cache_read_input_tokens',
    'cache_write_input_tokens', 'reasoning_output_tokens', 'total_tokens'];
  return Object.fromEntries(keys.filter(key => Number.isFinite(value[key])).map(key => [key, value[key]]));
}
`);
  patchFile("src/parsers/codex.js", [
    [`import {
  closeSync,`, `import { normalizeUsageRecord } from './codex-usage-record.js';
import {
  closeSync,`, "Codex durable usage compatibility"],
    ["const obj = JSON.parse(line);", "const obj = normalizeUsageRecord(JSON.parse(line));", "Codex durable usage compatibility"],
    ["  let prevCumulativeTotal = previousTail?.prevCumulativeTotal ?? null;", `  let prevCumulativeTotal = previousTail?.prevCumulativeTotal ?? null;
  let prevRecordTotal = previousTail?.prevRecordTotal ?? null;
  let pendingUsageMirror = previousTail?.pendingUsageMirror ?? null;`, "Codex durable usage compatibility"],
    ["      parsedRecordIndex++;", `      parsedRecordIndex++;
      // A resumed/new turn cannot be the delayed mirror of the preceding request.
      if (obj.type === 'session_meta' || obj.type === 'turn_context' || obj.type === 'token_usage_record'
          || (obj.type === 'event_msg' && isTaskStarted(obj.payload))) pendingUsageMirror = null;`, "Codex durable usage compatibility"],
    ["      const cumulativeTotal = info.total_token_usage?.total_tokens;", `      const isUsageRecord = payload.usage_record === true;
      // Durable and UI cumulative counters may diverge after an interrupted call.
      // Match their per-request usage instead, retaining the marker across appends.
      const isMirror = !isUsageRecord && pendingUsageMirror && sameRequestUsage(pendingUsageMirror, info.last_token_usage);
      pendingUsageMirror = isUsageRecord ? info.last_token_usage : null;
      const cumulativeTotal = info.total_token_usage?.total_tokens;`, "Codex durable usage compatibility"],
    ["&& cumulativeTotal === prevCumulativeTotal;", "&& cumulativeTotal === (isUsageRecord ? prevRecordTotal : prevCumulativeTotal);", "Codex durable usage compatibility"],
    ["      if (typeof cumulativeTotal === 'number') prevCumulativeTotal = cumulativeTotal;", `      if (typeof cumulativeTotal === 'number') {
        if (isUsageRecord) prevRecordTotal = cumulativeTotal;
        else prevCumulativeTotal = cumulativeTotal;
      }`, "Codex durable usage compatibility"],
    ["if (isReplayedHistory || isDuplicateEmission) continue;", "if (isReplayedHistory || isDuplicateEmission || isMirror) continue;", "Codex durable usage compatibility"],
    ["      prevCumulativeTotal,", `      prevCumulativeTotal,
      prevRecordTotal,
      pendingUsageMirror,`, "Codex durable usage compatibility"],
    ["export async function parse(options = {}) {", `function sameRequestUsage(left, right) {
  if (!left || !right) return false;
  return ['input_tokens', 'cached_input_tokens', 'output_tokens', 'reasoning_output_tokens']
    .every(key => (left[key] || 0) === (right[key] || 0));
}

export async function parse(options = {}) {`, "Codex durable usage compatibility"],
  ]);
  patchFile("src/parsers/codex-segments.js", [
    ["import { createHash } from 'node:crypto';", `import { createHash } from 'node:crypto';
import { normalizeUsageRecord } from './codex-usage-record.js';`, "Codex durable usage compatibility"],
    ["function accountingRecord(obj, context) {", `function accountingRecord(obj, context) {
  obj = normalizeUsageRecord(obj);`, "Codex durable usage compatibility"],
    ["['type', 'started_at', 'model']", "['type', 'started_at', 'model', 'usage_record']", "Codex durable usage compatibility"],
  ]);
  patchFile("src/parsers/codex-cache.js", [
    ["CODEX_PARSER_ALGORITHM_VERSION = 4", "CODEX_PARSER_ALGORITHM_VERSION = 5", "Codex durable usage compatibility"],
  ]);
}

applyWindowsPatches();
applyCodexUsageRecordPatch();

const pkg = JSON.parse(fs.readFileSync(path.join(destDir, "package.json"), "utf8"));
if (pkg.name !== "@vibe-cafe/vibe-usage" || !/^\d+\.\d+\.\d+(?:[-+].+)?$/.test(pkg.version)) {
  throw new Error(`invalid vendored CLI identity: ${pkg.name}@${pkg.version}`);
}
// Provenance of the snapshot itself, written here so it can never disagree with
// the bytes; the Windows external-test diagnostics binary embeds this file.
fs.writeFileSync(
  path.join(destDir, ".vibe-usage-source.json"),
  `${JSON.stringify({ version: pkg.version, ...sourceMetadata }, null, 2)}\n`,
);
log(`recorded source: ${sourceMetadata.source}${sourceMetadata.commit ? ` ${sourceMetadata.commit}` : " (registry release)"}`);
log(`resolved @${CLI_CHANNEL} to ${pkg.name}@${pkg.version} → ${path.relative(root, destDir)}`);
