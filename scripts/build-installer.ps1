# Builds the Windows NSIS installer (app + oxyd-bridge.exe sidecar).
# Usage: powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$triple = (rustc -vV | Select-String "^host: (.+)$").Matches[0].Groups[1].Value
Write-Host "==> building oxyd-bridge ($triple)"
cargo build --release -p oxyd-bridge
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$bin = Join-Path $root "app\src-tauri\binaries"
New-Item -ItemType Directory -Force $bin | Out-Null
Copy-Item "target\release\oxyd-bridge.exe" (Join-Path $bin "oxyd-bridge-$triple.exe") -Force

Write-Host "==> building extension"
Push-Location extension
npm ci
npm run build
Pop-Location

Write-Host "==> building installer"
Push-Location app
npm ci
npx tauri build --config src-tauri/tauri.bundle.json
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
Pop-Location

Get-ChildItem "target\release\bundle\nsis\*.exe" | ForEach-Object {
    Write-Host ("installer: {0} ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB))
}
