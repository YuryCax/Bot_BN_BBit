# Phase 0 — Edge Research collector (§9.0)
# Default: 7 days continuous collection for BTC + ETH

param(
    [int]$DurationSec = 604800,
    [int]$FlushIntervalSec = 3600,
    [string[]]$Symbols = @("BTCUSDT", "ETHUSDT"),
    [string]$Output = "research/data"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

$python = "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe"
if (-not (Test-Path $python)) { $python = "python" }

Write-Host "Starting Edge Research collector for $DurationSec sec..."
& $python research/collector/collector.py `
    --symbols $Symbols `
    --output $Output `
    --duration-sec $DurationSec `
    --flush-interval-sec $FlushIntervalSec

Write-Host "Running analyze..."
& $python research/edge_report/analyze.py --data-dir $Output

Write-Host "Done. Check config/edge_profile.toml meta.status"
