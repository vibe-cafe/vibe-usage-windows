// Renders the Settings window off screen, one PNG per state, for design
// review on any machine (the Tauri shell itself only runs on Windows):
//
//     node scripts/settings-preview/render.mjs <output dir> [state …]
//
// Run from the repository root (Tailwind resolves its content paths there).
// Builds `preview.tsx` (fixtures only, see there) into a temporary folder,
// serves it on localhost and takes each screenshot with headless Chrome at
// 2× — no window is shown and nothing on screen is captured. Set CHROME to
// the browser binary if it is not at the default macOS location.

import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";
import react from "@vitejs/plugin-react";

const here = dirname(fileURLToPath(import.meta.url));
const [outArg, ...only] = process.argv.slice(2);
if (!outArg) {
  console.error("usage: node scripts/settings-preview/render.mjs <output dir> [state …]");
  process.exit(2);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });

const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

/** State → window size (CSS px). 700 × 460 is the default window; taller
 *  sizes show a whole page, like the macOS render tests. */
const STATES = {
  general: [700, 460],
  quotas: [700, 560],
  "quotas-default": [700, 460, "quotas"],
  "quotas-zcode": [700, 600],
  "quotas-zcode-saved": [700, 600],
  "quotas-zcode-error": [700, 620],
  "quotas-cursor": [700, 600],
  "quotas-cursor-signed-in": [700, 600],
  "quotas-cursor-expired": [700, 600],
  "quotas-status": [700, 560],
  "quotas-menu": [700, 660],
  "quotas-drag": [700, 560],
  "quotas-from-panel": [700, 600],
  "quotas-narrow": [620, 560, "quotas"],
  sync: [700, 460],
  "sync-error": [700, 460],
  "sync-relinking": [700, 460],
  "sync-unlinked": [700, 460],
  "sync-reset": [700, 460],
  directories: [700, 460],
  "directories-empty": [700, 460],
  "directories-roots": [700, 520],
  about: [700, 620],
  "about-default": [700, 460, "about"],
  "about-copied": [700, 760],
};

const work = mkdtempSync(join(tmpdir(), "settings-preview-"));
const dist = join(work, "dist");
await build({
  root: here,
  base: "./",
  configFile: false,
  logLevel: "error",
  plugins: [react()],
  build: { outDir: dist, emptyOutDir: true, target: "chrome105" },
});

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".woff": "font/woff", ".woff2": "font/woff2" };
const server = createServer(async (request, response) => {
  const path = normalize(decodeURIComponent(new URL(request.url, "http://localhost").pathname));
  try {
    const body = await readFile(join(dist, path === "/" ? "index.html" : path));
    response.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream" });
    response.end(body);
  } catch {
    response.writeHead(404);
    response.end();
  }
});
await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
const { port } = server.address();
const profile = join(work, "chrome");

function chrome(args) {
  return new Promise((done) => {
    const child = spawn(CHROME, args, { stdio: ["ignore", "ignore", "pipe"] });
    let stderr = "";
    child.stderr.on("data", (chunk) => (stderr += chunk));
    const timer = setTimeout(() => child.kill("SIGKILL"), 60_000);
    child.on("exit", (code) => {
      clearTimeout(timer);
      done({ code, stderr });
    });
  });
}

try {
  for (const [name, [width, height, state = name]] of Object.entries(STATES)) {
    if (only.length && !only.includes(name)) continue;
    const file = join(out, `${name}.png`);
    const result = await chrome([
      "--headless=new",
      "--disable-gpu",
      "--hide-scrollbars",
      "--no-first-run",
      "--no-default-browser-check",
      `--user-data-dir=${profile}`,
      "--force-device-scale-factor=2",
      `--window-size=${width},${height}`,
      "--virtual-time-budget=6000",
      `--screenshot=${file}`,
      `http://127.0.0.1:${port}/index.html?state=${encodeURIComponent(state)}`,
    ]);
    if (result.code !== 0) {
      console.error(`${name}: chrome exited ${result.code}\n${result.stderr}`);
    } else {
      console.log(`${name}.png`);
    }
  }
} finally {
  server.close();
  rmSync(work, { recursive: true, force: true });
}
