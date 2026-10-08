#!/usr/bin/env node
// Version consistency gate (counterpart of macOS scripts/check-version.sh):
// the three app version locations must agree.
//
// There is no CLI version to check any more. The app runs
// `@vibe-cafe/vibe-usage@latest` through `src-tauri/resources/cli-bootstrap.mjs`,
// which resolves the registry's `latest` dist-tag at run time and caches what it
// got — a client-side pin rots silently and freezes users out of every CLI fix,
// the same policy the macOS app follows. `check-cli.mjs` in the macOS repo is
// the pattern for validating the *published* package's contracts; this app
// cannot check a version offline because it deliberately does not pin one.
//
// This script is offline and only reads the app manifests.

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

console.log(`package.json:     ${pkg}`);
console.log(`tauri.conf.json:  ${tauri}`);
console.log(`Cargo.toml:       ${cargo}`);
console.log(`CLI channel:      ${cliChannel}`);

if (pkg !== tauri || pkg !== cargo) {
  console.error("✗ version mismatch — update all three before releasing");
  process.exit(1);
}
// The channel is the one CLI setting a release can get wrong: anything but
// `latest` would be a pin, and the launcher reads the dist-tag itself.
if (cliChannel !== "latest") {
  console.error("✗ CLI channel must be latest");
  process.exit(1);
}
if (packageJson.vibeUsageCliVersion !== undefined) {
  console.error("✗ vibeUsageCliVersion must not come back — the app resolves latest at run time");
  process.exit(1);
}
if (!fs.existsSync(path.join(root, "src-tauri/resources/cli-bootstrap.mjs"))) {
  console.error("✗ src-tauri/resources/cli-bootstrap.mjs is missing — the app has no way to run the CLI");
  process.exit(1);
}
console.log("✓ app versions agree; CLI is resolved from the registry's latest at run time");
