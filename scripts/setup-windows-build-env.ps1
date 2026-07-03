# One-shot Windows build environment setup (winget based, adapted from ATM).
#   pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/setup-windows-build-env.ps1

$ErrorActionPreference = "Stop"

function Ensure-Tool($name, $wingetId, $check) {
    if (Get-Command $check -ErrorAction SilentlyContinue) {
        Write-Host "✓ $name already installed" -ForegroundColor Green
        return
    }
    Write-Host "Installing $name..." -ForegroundColor Cyan
    winget install --id $wingetId -e --accept-source-agreements --accept-package-agreements
}

Ensure-Tool "Node.js LTS" "OpenJS.NodeJS.LTS" "node"
Ensure-Tool "Rustup" "Rustlang.Rustup" "rustup"
Ensure-Tool "Visual Studio Build Tools" "Microsoft.VisualStudio.2022.BuildTools" "cl"

rustup toolchain install 1.88.0 --profile minimal
npm install -g pnpm@10

Write-Host "`nEnvironment ready. Run:" -ForegroundColor Green
Write-Host "  pnpm install"
Write-Host "  pnpm run release:windows"
