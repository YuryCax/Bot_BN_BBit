# Phase 0.5 — Quant Hardening (L2 + CCF)
# Usage:
#   .\scripts\run_quant_hardening.ps1
#   .\scripts\run_quant_hardening.ps1 -Days 14 -LiveDownload
#   .\scripts\run_quant_hardening.ps1 -Days 14 -LiveDownload -Symbols BTCUSDT,ETHUSDT

param(
    [int]$Days = 14,
    [switch]$LiveDownload,
    [double]$NotionalUsd = 3000,
    [string[]]$Symbols = @("BTCUSDT", "ETHUSDT")
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# Normalize "BTCUSDT,ETHUSDT" → two symbols (PowerShell sometimes passes one string)
$Symbols = @(
    $Symbols |
        ForEach-Object { $_ -split "," } |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ }
)

$python = "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe"
if (-not (Test-Path $python)) { $python = "python" }

Write-Host "Installing quant deps..."
& $python -m pip install -q -r research/quant/requirements.txt
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$hist = "research/data/hist"
$symArgs = @("--symbols") + $Symbols
Write-Host "Symbols: $($Symbols -join ' | ')"

if ($LiveDownload) {
    Write-Host "Downloading Binance Vision + Bybit public trades ($Days days)..."
    & $python research/quant/download_hist.py --days $Days --output $hist --live-depth @symArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} else {
    Write-Host "Generating synthetic hist fixtures (offline)..."
    & $python research/quant/download_hist.py --days $Days --output $hist --synthetic @symArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "Running L2-aware analyze_lead_lag..."
$dataSourceArg = if ($LiveDownload) {
    @("--data-source", "binance_vision")
} else {
    @("--data-source", "synthetic")
}
& $python research/quant/analyze_lead_lag.py `
    --hist-dir $hist `
    --notional-usd $NotionalUsd `
    --output-profile config/edge_profile.toml `
    --output-summary research/edge_report/summary.md `
    --output-params research/edge_report/params_for_rust.json `
    --symbols-toml config/symbols.toml `
    --symbols @Symbols `
    @dataSourceArg
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Done. Check config/edge_profile.toml and symbols.toml"
