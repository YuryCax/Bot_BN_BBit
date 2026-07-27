# Mono-node wiring smoke (mode=dev) — ADR-003.
# Runs executor + observer briefly; checks heartbeat/ticks path artifacts.
# Does NOT require edge pass. Does NOT place live orders.

param(
    [int]$Seconds = 45,
    [string]$Config = "config/config.toml"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# Refuse paper/live for this smoke
$cfgText = Get-Content $Config -Raw
if ($cfgText -match 'mode\s*=\s*"(paper|live|start)"') {
    Write-Error "Smoke refuses mode=paper/live/start. Keep mode=dev until edge status=pass."
}

$env:BOT_CONFIG = $Config
$env:BOT_SYMBOLS = "config/symbols.toml"
$env:BOT_PACKET_LOG = "logs/smoke_packets.bin"
$env:BOT_LEDGER = "logs/smoke_paper_ledger.jsonl"

New-Item -ItemType Directory -Force -Path logs | Out-Null
Remove-Item -ErrorAction SilentlyContinue $env:BOT_PACKET_LOG, $env:BOT_LEDGER, "logs/paper_summary.txt"

Write-Host "Building release (observer + executor)..."
& "$root\scripts\build.ps1" -Extra " -p observer-bin -p executor-bin"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exeDir = if ($env:CARGO_TARGET_DIR) {
    Join-Path $env:CARGO_TARGET_DIR "release"
} else {
    Join-Path $root "target\release"
}
$exe = Join-Path $exeDir "executor.exe"
$obs = Join-Path $exeDir "observer.exe"
if (-not (Test-Path $exe) -or -not (Test-Path $obs)) {
    Write-Error "Missing release binaries under $exeDir"
}
Write-Host "Using binaries from $exeDir"

Write-Host "Starting executor (dev dry-run)..."
$pEx = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 3
if ($pEx.HasExited) {
    Write-Error "executor exited early code=$($pEx.ExitCode)"
}

Write-Host "Starting observer..."
$pOb = Start-Process -FilePath $obs -WorkingDirectory $root -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 3
if ($pOb.HasExited) {
    Stop-Process -Id $pEx.Id -Force -ErrorAction SilentlyContinue
    Write-Error "observer exited early code=$($pOb.ExitCode)"
}

Write-Host "Running smoke for $Seconds s (WS + Zenoh)..."
Start-Sleep -Seconds $Seconds

Stop-Process -Id $pOb.Id -Force -ErrorAction SilentlyContinue
Stop-Process -Id $pEx.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$ok = $true
$pkt = Get-Item -ErrorAction SilentlyContinue $env:BOT_PACKET_LOG
if (-not $pkt -or $pkt.Length -lt 32) {
    Write-Warning "packet log missing/small: $($env:BOT_PACKET_LOG)"
    $ok = $false
} else {
    Write-Host "OK packet log bytes=$($pkt.Length)"
}

# Ledger may be empty if no entries — that is acceptable for wiring smoke
Write-Host "Smoke complete. ok_packets=$ok"
if (-not $ok) { exit 2 }
exit 0
