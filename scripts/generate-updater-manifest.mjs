#!/usr/bin/env node
// Generate latest.json (update manifest) from a built NSIS installer.
// Counterpart of the macOS generate-appcast.sh (Sparkle appcast).
//
// Usage: node scripts/generate-updater-manifest.mjs <installer.exe> [notes]

import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const version = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).version;

const installer = process.argv[2];
if (!installer || !fs.existsSync(installer)) {
  console.error("usage: node scripts/generate-updater-manifest.mjs <installer.exe> [notes]");
  process.exit(1);
}
const notes = process.argv[3] ?? `Vibe Usage for Windows v${version}`;

const sha256 = createHash("sha256").update(fs.readFileSync(installer)).digest("hex");
const repo = process.env.GITHUB_REPOSITORY ?? "kalasoo/vibe-usage-windows";
const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  url: `https://github.com/${repo}/releases/download/v${version}/${path.basename(installer)}`,
  sha256,
};

const out = path.join(path.dirname(installer), "latest.json");
fs.writeFileSync(out, JSON.stringify(manifest, null, 2));
console.log(`[manifest] wrote ${out}`);
console.log(JSON.stringify(manifest, null, 2));
