# Run R1 analyze + sweep without LiveDownload (uses existing hist).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

$python = Join-Path $env:LOCALAPPDATA "Programs\Python\Python312\python.exe"
if (-not (Test-Path $python)) { $python = "python" }

Write-Host "R1 analyze (event+bar)..."
& $python research/quant/analyze_lead_lag.py `
    --hist-dir research/data/hist `
    --symbols BTCUSDT ETHUSDT `
    --method both `
    --data-source binance_vision
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "R1 sweep..."
& $python research/quant/sweep_edge.py `
    --hist-dir research/data/hist `
    --symbols BTCUSDT ETHUSDT
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "R1 done."
