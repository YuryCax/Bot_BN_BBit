# Mono-node ADR-003: in-process dual path on one host (dev only).
# Starts Executor (entry+risk+local Bybit) then Observer (thin Binance forwarder).
# Requires Zenoh peer connectivity on localhost. Not valid for live go/no-go (§2.6).

param(
    [string]$Config = "config/config.toml"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

if (Test-Path (Join-Path $root "secrets.env")) {
    & "$root\scripts\Import-BotSecrets.ps1"
}

$env:BOT_CONFIG = $Config
$env:BOT_SYMBOLS = "config/symbols.toml"
$env:BOT_PACKET_LOG = "logs/packets.bin"
$env:BOT_LEDGER = "logs/paper_ledger.jsonl"

$exeDir = if ($env:CARGO_TARGET_DIR) {
    Join-Path $env:CARGO_TARGET_DIR "release"
} else {
    Join-Path $root "target\release"
}
$exe = Join-Path $exeDir "executor.exe"
$obs = Join-Path $exeDir "observer.exe"

Write-Host "Building release..."
& "$root\scripts\build.ps1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (-not (Test-Path $exe) -or -not (Test-Path $obs)) {
    Write-Error "Missing release binaries under $exeDir"
}

Write-Host "Starting executor (Singapore role: Entry + Risk) from $exeDir..."
Start-Process -FilePath $exe -WorkingDirectory $root -NoNewWindow

Start-Sleep -Seconds 2
Write-Host "Starting observer (Tokyo role: BinanceTick forwarder + heartbeat)..."
& $obs
