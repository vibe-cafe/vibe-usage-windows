#!/usr/bin/env node
// Version consistency gate (counterpart of macOS scripts/check-version.sh):
// package.json ↔ src-tauri/tauri.conf.json ↔ Cargo workspace version.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => fs.readFileSync(path.join(root, p), "utf8");

const pkg = JSON.parse(read("package.json")).version;
const tauri = JSON.parse(read("src-tauri/tauri.conf.json")).version;
const cargo = /\[workspace\.package\][^[]*?version\s*=\s*"([^"]+)"/s.exec(read("Cargo.toml"))?.[1];

console.log(`package.json:     ${pkg}`);
console.log(`tauri.conf.json:  ${tauri}`);
console.log(`Cargo.toml:       ${cargo}`);

if (pkg !== tauri || pkg !== cargo) {
  console.error("✗ version mismatch — update all three before releasing");
  process.exit(1);
}
console.log("✓ versions consistent");
