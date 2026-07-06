# Local Windows release build (mirror of the GitHub Actions release job).
# Prereqs: Node 22+, pnpm 10, Rust 1.88 (rustup), NSIS (bundled with tauri-cli).
#
#   pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/release-windows.ps1

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

Write-Host "== Vibe Usage for Windows release build ==" -ForegroundColor Cyan

node scripts/check-version.mjs
if ($LASTEXITCODE -ne 0) { exit 1 }

pnpm install --frozen-lockfile
if ($LASTEXITCODE -ne 0) { exit 1 }

pnpm test
if ($LASTEXITCODE -ne 0) { exit 1 }

cargo test --workspace
if ($LASTEXITCODE -ne 0) { exit 1 }

node scripts/vendor-cli.mjs
if ($LASTEXITCODE -ne 0) { exit 1 }

node scripts/fetch-node.mjs
if ($LASTEXITCODE -ne 0) { exit 1 }

& (Join-Path $PSScriptRoot "build-tauri-windows.ps1")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$version = (Get-Content package.json | ConvertFrom-Json).version
$installer = Get-ChildItem -Path "target/release/bundle/nsis" -Filter "*$version*setup.exe" |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if (-not $installer) {
  throw "No NSIS installer found for version $version."
}
$dest = "VibeUsage-$version-Windows-Setup.exe"
Copy-Item $installer.FullName $dest -Force
node scripts/generate-updater-manifest.mjs $dest

Write-Host "`n✓ $dest" -ForegroundColor Green
Write-Host "✓ latest.json" -ForegroundColor Green
