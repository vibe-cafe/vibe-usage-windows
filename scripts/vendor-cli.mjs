#!/usr/bin/env node
// Vendor the @vibe-cafe/vibe-usage CLI into src-tauri/resources/cli and apply
// the Windows patches (upstreamed via the windows-support PR; vendored copies
// stay patched so releases don't depend on upstream merge timing).
//
// Usage:
//   node scripts/vendor-cli.mjs                  # npm pack @vibe-cafe/vibe-usage@<pinned>
//   node scripts/vendor-cli.mjs --from-local ../vibe-usage   # copy a local checkout
//
// The CLI has zero npm dependencies, so vendoring bin/ + src/ + package.json
// is sufficient — no node_modules.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CLI_VERSION = "0.9.13"; // bump deliberately; releases go through regression tests

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
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
}

function vendorFromNpm() {
  log(`npm pack @vibe-cafe/vibe-usage@${CLI_VERSION}`);
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "vibe-cli-"));
  try {
    const out = execFileSync(
      "npm",
      ["pack", `@vibe-cafe/vibe-usage@${CLI_VERSION}`, "--pack-destination", tmp],
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

  // 1b. fetchSettings runs OUTSIDE sync.js's try/catch — a network failure
  // (offline, proxy required) crashes Node with an uncaught exception instead
  // of a clean "同步失败" message. Wrap it; on failure default to NOT
  // uploading project names (the privacy-safe direction).
  patchFile("src/sync.js", [
    [
      "  const settings = await fetchSettings(apiUrl, config.apiKey);",
      `  let settings = null;
  try {
    settings = await fetchSettings(apiUrl, config.apiKey);
  } catch (err) {
    process.stderr.write(\`\${dim(\`  settings: \${err.message}（默认隐藏项目名）\`)}\\n\`);
  }`,
      "sync fetchSettings crash guard",
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

  // 3. OpenCode on Windows stores data under %LOCALAPPDATA%\\opencode.
  patchFile("src/parsers/opencode.js", [
    [
      "const DATA_DIR = join(homedir(), '.local', 'share', 'opencode');",
      `function resolveOpencodeDataDir() {
  const xdg = join(homedir(), '.local', 'share', 'opencode');
  if (process.platform === 'win32' && !existsSync(xdg) && process.env.LOCALAPPDATA) {
    const winDir = join(process.env.LOCALAPPDATA, 'opencode');
    if (existsSync(winDir)) return winDir;
  }
  return xdg;
}
const DATA_DIR = resolveOpencodeDataDir();`,
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
      "import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';",
      "import { readFileSync, writeFileSync, mkdirSync, existsSync, renameSync, statSync } from 'node:fs';",
      "config fs helpers",
    ],
    [
      "const CONFIG_DIR = join(homedir(), '.vibe-usage');",
      "const CONFIG_DIR = process.env.VIBE_USAGE_CONFIG_DIR?.trim() || join(homedir(), '.vibe-usage');",
      "config app dir override",
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
      "  try {\n    return JSON.parse(readFileSync(CONFIG_FILE, 'utf-8'));",
      "  try {\n    if (!statSync(CONFIG_FILE).isFile()) return null;\n    return JSON.parse(readFileSync(CONFIG_FILE, 'utf-8'));",
      "config ignore non-file",
    ],
    [
      "  mkdirSync(CONFIG_DIR, { recursive: true });\n  writeFileSync(CONFIG_FILE, JSON.stringify(config, null, 2) + '\\n', 'utf-8');",
      "  mkdirSync(CONFIG_DIR, { recursive: true });\n  moveDirectoryOutOfFilePath(CONFIG_FILE);\n  writeFileSync(CONFIG_FILE, JSON.stringify(config, null, 2) + '\\n', 'utf-8');",
      "config save EISDIR repair",
    ],
  ]);

  patchFile("src/state.js", [
    [
      "import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';",
      "import { readFileSync, writeFileSync, mkdirSync, existsSync, renameSync, statSync } from 'node:fs';",
      "state fs helpers",
    ],
    [
      "const STATE_DIR = join(homedir(), '.vibe-usage');",
      "const STATE_DIR = process.env.VIBE_USAGE_CONFIG_DIR?.trim() || join(homedir(), '.vibe-usage');",
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
      "  try {\n    const parsed = JSON.parse(readFileSync(STATE_FILE, 'utf-8'));",
      "  try {\n    if (!statSync(STATE_FILE).isFile()) return { buckets: {}, sessions: {} };\n    const parsed = JSON.parse(readFileSync(STATE_FILE, 'utf-8'));",
      "state ignore non-file",
    ],
    [
      "  mkdirSync(STATE_DIR, { recursive: true });\n  writeFileSync(STATE_FILE, JSON.stringify(state) + '\\n', 'utf-8');",
      "  mkdirSync(STATE_DIR, { recursive: true });\n  moveDirectoryOutOfFilePath(STATE_FILE);\n  writeFileSync(STATE_FILE, JSON.stringify(state) + '\\n', 'utf-8');",
      "state save EISDIR repair",
    ],
  ]);
}

// ---------------------------------------------------------------------------

const localFlag = process.argv.indexOf("--from-local");
if (localFlag >= 0) {
  vendorFromLocal(process.argv[localFlag + 1] ?? "../vibe-usage");
} else {
  try {
    vendorFromNpm();
  } catch (err) {
    log(`npm pack failed (${err.message}); falling back to ../vibe-usage`);
    vendorFromLocal("../vibe-usage");
  }
}

applyWindowsPatches();

const pkg = JSON.parse(fs.readFileSync(path.join(destDir, "package.json"), "utf8"));
log(`vendored ${pkg.name}@${pkg.version} → ${path.relative(root, destDir)}`);
