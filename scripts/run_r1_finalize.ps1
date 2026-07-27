# Quick R1 re-analyze with honest mean_net gate (existing hist).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$python = Join-Path $env:LOCALAPPDATA "Programs\Python\Python312\python.exe"
if (-not (Test-Path $python)) { $python = "python" }

& $python research/quant/analyze_lead_lag.py `
    --hist-dir research/data/hist `
    --symbols BTCUSDT ETHUSDT `
    --method event `
    --impulse-min-bps 12 `
    --latency-ms 150 `
    --bar-ms 50 `
    --data-source binance_vision
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Default baseline params for profile used by bot
& $python research/quant/analyze_lead_lag.py `
    --hist-dir research/data/hist `
    --symbols BTCUSDT ETHUSDT `
    --method event `
    --data-source binance_vision
exit $LASTEXITCODE
