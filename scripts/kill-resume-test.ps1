# Robustness test: kill the CLI (taskkill /F) at random moments during a throttled
# download, resume, repeat, and finally verify the file byte for byte.
#
#   powershell -ExecutionPolicy Bypass -File scripts\kill-resume-test.ps1 [-Runs 100] [-SizeMB 32]
param(
    [int]$Runs = 100,
    [int]$SizeMB = 32,
    [int]$Port = 8790
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
cargo build --release -p oxyd-cli -p test-server
if ($LASTEXITCODE -ne 0) { throw "build failed" }

$work = Join-Path ([System.IO.Path]::GetTempPath()) "oxyd-kill-test"
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
$db = Join-Path $work "cli.db"
$size = $SizeMB * 1MB
# Throttle each connection so a run lasts long enough to be interrupted.
$url = "http://127.0.0.1:$Port/file/${size}?rate=524288&name=kill.bin"

$server = Start-Process "target\release\test-server.exe" -ArgumentList $Port -PassThru -WindowStyle Hidden
Start-Sleep -Milliseconds 500
$cli = "target\release\oxyd.exe"
$rand = New-Object System.Random
$kills = 0
try {
    $p = Start-Process $cli -ArgumentList "--db `"$db`" download `"$url`" -o `"$work`" -c 8" -PassThru -WindowStyle Hidden
    for ($i = 1; $i -le $Runs; $i++) {
        Start-Sleep -Milliseconds ($rand.Next(150, 1200))
        if ($p.HasExited) { break }
        taskkill /F /PID $p.Id | Out-Null
        $kills++
        $id = (& $cli --db $db list | Select-Object -First 1).Substring(0, 8)
        $p = Start-Process $cli -ArgumentList "--db `"$db`" resume $id" -PassThru -WindowStyle Hidden
    }
    $p.WaitForExit()
    & $cli --db $db list
} finally {
    Stop-Process $server -Force -ErrorAction SilentlyContinue
}

# The test server serves byte i = (i % 251) XOR ((i >> 12) & 0xFF).
Add-Type -TypeDefinition @"
public static class OxydCheck {
    public static long FirstBadOffset(byte[] d) {
        for (long i = 0; i < d.Length; i++) {
            if (d[i] != (byte)((i % 251) ^ ((i >> 12) & 0xFF))) return i;
        }
        return -1;
    }
}
"@
$data = [System.IO.File]::ReadAllBytes((Join-Path $work "kill.bin"))
if ($data.Length -ne $size) { throw "size mismatch: $($data.Length) != $size" }
$bad = [OxydCheck]::FirstBadOffset($data)
if ($bad -ge 0) { throw "corrupt byte at offset $bad after $kills kills" }
Write-Host "OK: $kills kills, file intact ($SizeMB MB)"
