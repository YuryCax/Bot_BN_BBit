# Bybit authenticated order smoke (TESTNET by default).
# Prerequisites: .\scripts\init_secrets.ps1 then fill BYBIT_* in secrets.env

param(
    [string]$Symbol = "BTCUSDT",
    [int]$Leverage = 3
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

& "$root\scripts\Import-BotSecrets.ps1"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (-not $env:BYBIT_API_KEY -or -not $env:BYBIT_API_SECRET) {
    Write-Error "BYBIT_API_KEY / BYBIT_API_SECRET empty in secrets.env"
}

if ($env:BYBIT_TESTNET -ne "1" -and $env:BOT_ALLOW_MAINNET_SMOKE -ne "1") {
    Write-Error "Set BYBIT_TESTNET=1 in secrets.env for this smoke"
}

$env:SMOKE_SYMBOL = $Symbol
$env:SMOKE_LEVERAGE = "$Leverage"

Write-Host "Building smoke-bybit..."
& "$root\scripts\build.ps1" -Extra " -p smoke-bybit"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exeDir = if ($env:CARGO_TARGET_DIR) {
    Join-Path $env:CARGO_TARGET_DIR "release"
} else {
    Join-Path $root "target\release"
}
$bin = Join-Path $exeDir "smoke-bybit.exe"
if (-not (Test-Path $bin)) { $bin = Join-Path $exeDir "smoke-bybit" }

Write-Host "Running $bin (symbol=$Symbol)..."
& $bin
exit $LASTEXITCODE
