# Builds the Windows installers (app + oxyd-bridge.exe sidecar):
#   - an NSIS setup .exe (per user, no administrator rights)
#   - an .msi (per machine, for deployment tools and Group Policy)
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

# The MSI registers the native messaging host through a WiX fragment
# (installer\native-host.wxs). Its manifest comes from the shared template with a path
# relative to the manifest, so it works in whatever folder the MSI is installed to.
$hostDir = Join-Path $root "app\src-tauri\native-host"
New-Item -ItemType Directory -Force $hostDir | Out-Null
(Get-Content "installer\native-host\com.oxyd.bridge.json.template" -Raw).Replace("__BRIDGE_PATH__", "oxyd-bridge.exe") |
    Set-Content (Join-Path $hostDir "com.oxyd.bridge.json") -Encoding ascii

# MSI versions must be numeric, so pre-release versions (0.2.0-beta.1) only get the NSIS setup.
$version = (Get-Content "app\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version
$bundles = if ($version -match "-") { "nsis" } else { "nsis,msi" }

Write-Host "==> building installers ($bundles)"
Push-Location app
npm ci
npx tauri build --bundles $bundles --config src-tauri/tauri.bundle.json
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
Pop-Location

Get-ChildItem "target\release\bundle\nsis\*.exe", "target\release\bundle\msi\*.msi" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host ("installer: {0} ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB))
}
