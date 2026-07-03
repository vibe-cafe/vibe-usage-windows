#!/usr/bin/env node
// Download the bundled Node.js runtime (win-x64 node.exe) into
// src-tauri/resources/node/. Run before `tauri build` on Windows/CI.
// The bundle guarantees node:sqlite (Node ≥ 22.5) with zero user setup.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const NODE_VERSION = "22.23.1"; // keep in sync with docs/PARITY.md

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const destDir = path.join(root, "src-tauri", "resources", "node");
const dest = path.join(destDir, "node.exe");

function log(msg) {
  console.log(`[fetch-node] ${msg}`);
}

if (fs.existsSync(dest) && fs.statSync(dest).size > 10_000_000) {
  log(`node.exe already present (${fs.statSync(dest).size} bytes) — skipping`);
  process.exit(0);
}

const base = `https://nodejs.org/dist/v${NODE_VERSION}`;
const zipName = `node-v${NODE_VERSION}-win-x64.zip`;

log(`downloading ${base}/${zipName}`);
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "vibe-node-"));
const zipPath = path.join(tmp, zipName);

const shasums = await fetchText(`${base}/SHASUMS256.txt`);
const expected = shasums
  .split("\n")
  .map((l) => l.trim().split(/\s+/))
  .find(([, name]) => name === zipName)?.[0];
if (!expected) {
  console.error(`[fetch-node] ${zipName} not found in SHASUMS256.txt`);
  process.exit(1);
}

await fetchToFile(`${base}/${zipName}`, zipPath);

const actual = createHash("sha256").update(fs.readFileSync(zipPath)).digest("hex");
if (actual !== expected) {
  console.error(`[fetch-node] SHA-256 mismatch: expected ${expected}, got ${actual}`);
  process.exit(1);
}
log("SHA-256 verified");

// Extract just node.exe.
if (process.platform === "win32") {
  execFileSync("tar", ["-xf", zipPath, "-C", tmp], { stdio: "inherit" });
} else {
  execFileSync("unzip", ["-q", "-o", zipPath, "-d", tmp], { stdio: "inherit" });
}
const extracted = path.join(tmp, `node-v${NODE_VERSION}-win-x64`, "node.exe");
fs.mkdirSync(destDir, { recursive: true });
fs.copyFileSync(extracted, dest);
fs.rmSync(tmp, { recursive: true, force: true });
log(`bundled node.exe v${NODE_VERSION} → ${path.relative(root, dest)}`);

async function fetchText(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`GET ${url} → ${res.status}`);
  return res.text();
}

async function fetchToFile(url, file) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`GET ${url} → ${res.status}`);
  fs.writeFileSync(file, Buffer.from(await res.arrayBuffer()));
}
