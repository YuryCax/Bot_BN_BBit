# Mono-node dev: executor + observer (requires edge pass or mode=dev in config)

param(
    [string]$Config = "config/config.toml"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

$env:BOT_CONFIG = $Config
$env:BOT_SYMBOLS = "config/symbols.toml"
$env:BOT_PACKET_LOG = "logs/packets.bin"

Write-Host "Building release..."
& "$root\scripts\build.ps1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Starting executor..."
Start-Process -FilePath "target\release\executor.exe" -WorkingDirectory $root -NoNewWindow

Start-Sleep -Seconds 2
Write-Host "Starting observer..."
& "target\release\observer.exe"
