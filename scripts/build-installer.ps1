# Builds the Windows installers (app + oxyd-bridge.exe sidecar):
#   - an NSIS setup .exe (per user, no administrator rights)
#   - an .msi (per machine, for deployment tools and Group Policy)
# Usage: powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
param([switch]$Http3)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

function Invoke-BuildCommand($Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

Push-Location $root
try {
    $hostLine = Invoke-BuildCommand rustc @("-vV") | Select-String "^host: (.+)$"
    if (-not $hostLine) { throw "rustc did not report a host target" }
    $triple = $hostLine.Matches[0].Groups[1].Value
    Write-Host "==> building oxyd-bridge ($triple)"
    Invoke-BuildCommand cargo @("build", "--locked", "--release", "-p", "oxyd-bridge")

    $bin = Join-Path $root "app\src-tauri\binaries"
    New-Item -ItemType Directory -Force $bin | Out-Null
    Copy-Item "target\release\oxyd-bridge.exe" (Join-Path $bin "oxyd-bridge-$triple.exe") -Force

    Write-Host "==> building extensions"
    Invoke-BuildCommand npm @("--prefix", "extension", "ci")
    Invoke-BuildCommand npm @("--prefix", "extension", "run", "build")

    # WiX uses a manifest with a path relative to the installed sidecar.
    $hostDir = Join-Path $root "app\src-tauri\native-host"
    New-Item -ItemType Directory -Force $hostDir | Out-Null
    (Get-Content "installer\native-host\com.oxyd.bridge.json.template" -Raw).Replace("__BRIDGE_PATH__", "oxyd-bridge.exe") |
        Set-Content (Join-Path $hostDir "com.oxyd.bridge.json") -Encoding ascii

    # MSI versions must be numeric, so pre-release versions only get the NSIS setup.
    $version = (Get-Content "app\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version
    $bundles = if ($version -match "-") { "nsis" } else { "nsis,msi" }
    Write-Host "==> building installers ($bundles)"
    $tauriArgs = @("tauri", "build", "--bundles", $bundles, "--config", "src-tauri/tauri.bundle.json")
    $previousRustFlags = $env:RUSTFLAGS
    Push-Location app
    try {
        Invoke-BuildCommand npm @("ci")
        if ($Http3) {
            $env:RUSTFLAGS = "$previousRustFlags --cfg reqwest_unstable".Trim()
            $tauriArgs += @("--features", "http3")
        }
        Invoke-BuildCommand npx $tauriArgs
    } finally {
        $env:RUSTFLAGS = $previousRustFlags
        Pop-Location
    }

    $installerPaths = @("target\release\bundle\nsis\*.exe")
    if ($bundles -eq "nsis,msi") { $installerPaths += "target\release\bundle\msi\*.msi" }
    Get-ChildItem $installerPaths -ErrorAction SilentlyContinue | ForEach-Object {
        Write-Host ("installer: {0} ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB))
    }
    Write-Host "Chromium extension: $(Join-Path $root 'extension\build')"
    Write-Host "Firefox extension: $(Join-Path $root 'extension\build-firefox')"
} finally {
    Pop-Location
}
