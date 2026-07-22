#!/usr/bin/env node
// Version consistency gate (counterpart of macOS scripts/check-version.sh):
// App versions must agree, and release vendoring must resolve npm's latest
// dist-tag to a concrete, self-contained CLI package.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => fs.readFileSync(path.join(root, p), "utf8");

const packageJson = JSON.parse(read("package.json"));
const pkg = packageJson.version;
const cliChannel = packageJson.vibeUsageCliChannel;
const tauri = JSON.parse(read("src-tauri/tauri.conf.json")).version;
const cargo = /\[workspace\.package\][^[]*?version\s*=\s*"([^"]+)"/s.exec(read("Cargo.toml"))?.[1];
const vendoredCli = JSON.parse(read("src-tauri/resources/cli/package.json")).version;

console.log(`package.json:     ${pkg}`);
console.log(`tauri.conf.json:  ${tauri}`);
console.log(`Cargo.toml:       ${cargo}`);
console.log(`CLI channel:      ${cliChannel}`);
console.log(`Vendored CLI:     ${vendoredCli}`);

if (pkg !== tauri || pkg !== cargo) {
  console.error("✗ version mismatch — update all three before releasing");
  process.exit(1);
}
if (cliChannel !== "latest") {
  console.error("✗ CLI channel must be latest");
  process.exit(1);
}
if (!/^\d+\.\d+\.\d+(?:[-+].+)?$/.test(vendoredCli)) {
  console.error("✗ vendored CLI must contain a concrete semantic version");
  process.exit(1);
}
console.log("✓ app versions and latest CLI channel are consistent");
