# Package release binaries + configs for AWS scp.
# Output: dist/bot-release-<utc>.tar.gz

param(
    [string]$OutDir = "dist"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

Write-Host "Building full release workspace..."
& "$root\scripts\build.ps1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$stage = Join-Path $root "$OutDir\stage-$stamp"
$binDir = Join-Path $stage "bin"
$cfgDir = Join-Path $stage "etc"
New-Item -ItemType Directory -Force -Path $binDir, $cfgDir | Out-Null

$exeDir = if ($env:CARGO_TARGET_DIR) {
    Join-Path $env:CARGO_TARGET_DIR "release"
} else {
    Join-Path $root "target\release"
}

$names = @("observer", "executor", "control-panel", "telegram-alerts", "replay", "smoke-bybit")
foreach ($n in $names) {
    $src = Join-Path $exeDir "$n.exe"
    if (-not (Test-Path $src)) { $src = Join-Path $exeDir $n }
    if (-not (Test-Path $src)) {
        Write-Warning "skip missing $n"
        continue
    }
    Copy-Item $src (Join-Path $binDir (Split-Path $src -Leaf))
}

Copy-Item (Join-Path $root "config\*.toml") $cfgDir
Copy-Item (Join-Path $root "deploy\zenoh-tokyo.json5.example") (Join-Path $cfgDir "zenoh-tokyo.json5.example")
Copy-Item (Join-Path $root "deploy\zenoh-singapore.json5.example") (Join-Path $cfgDir "zenoh-singapore.json5.example")
Copy-Item (Join-Path $root "secrets.env.example") (Join-Path $cfgDir "secrets.env.example")
Copy-Item (Join-Path $root "deploy\install.sh") (Join-Path $stage "install.sh")
Copy-Item -Recurse (Join-Path $root "deploy\systemd") (Join-Path $stage "systemd")

$tar = Join-Path $root "$OutDir\bot-release-$stamp.tar.gz"
New-Item -ItemType Directory -Force -Path (Join-Path $root $OutDir) | Out-Null

# Prefer tar (Windows 10+)
Push-Location (Join-Path $root $OutDir)
tar -czf "bot-release-$stamp.tar.gz" -C "stage-$stamp" .
Pop-Location
Remove-Item -Recurse -Force $stage

Write-Host "Created $tar"
Write-Host "Upload: scp $tar ubuntu@TOKYO:/tmp/"
Write-Host "        scp $tar ubuntu@SINGAPORE:/tmp/"
Write-Host "Then on each host: sudo bash /tmp/... see deploy/AWS_QUICKSTART.md"
