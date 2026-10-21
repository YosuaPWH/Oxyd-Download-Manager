# Registers a locally built oxyd-bridge.exe as the Chrome/Edge native messaging host for
# the current user (what the installer does). Use -Unregister to remove it again.
#
#   powershell -ExecutionPolicy Bypass -File scripts\register-dev.ps1 [-Bridge path] [-Unregister]
param(
    [string]$Bridge = "",
    [switch]$Unregister
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$hostName = "com.oxyd.bridge"
$dir = Join-Path $env:LOCALAPPDATA "Oxyd\native-host"
$manifest = Join-Path $dir "$hostName.json"
$keys = @(
    "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$hostName",
    "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$hostName"
)

if ($Unregister) {
    foreach ($k in $keys) { if (Test-Path $k) { Remove-Item $k -Recurse -Force } }
    if (Test-Path $manifest) { Remove-Item $manifest -Force }
    Write-Host "unregistered $hostName"
    exit 0
}

if (-not $Bridge) {
    $Bridge = Join-Path $root "target\debug\oxyd-bridge.exe"
    if (-not (Test-Path $Bridge)) {
        cargo build -p oxyd-bridge
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }
}
$Bridge = (Resolve-Path $Bridge).Path

$template = Get-Content (Join-Path $root "installer\native-host\$hostName.json.template") -Raw
$json = $template.Replace("__BRIDGE_PATH__", $Bridge.Replace("\", "\\"))
New-Item -ItemType Directory -Force $dir | Out-Null
[System.IO.File]::WriteAllText($manifest, $json)

foreach ($k in $keys) {
    New-Item -Path $k -Force | Out-Null
    Set-ItemProperty -Path $k -Name "(default)" -Value $manifest
}
Write-Host "registered $hostName -> $Bridge"
Write-Host "manifest: $manifest"
