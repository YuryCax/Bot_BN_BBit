# One-time: create gitignored secrets.env from example.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$dst = Join-Path $root "secrets.env"
$src = Join-Path $root "secrets.env.example"
if (Test-Path $dst) {
    Write-Host "Already exists: $dst"
    exit 0
}
Copy-Item $src $dst
Write-Host "Created $dst"
Write-Host "Fill BYBIT_API_KEY and BYBIT_API_SECRET (testnet), then run:"
Write-Host "  .\scripts\Import-BotSecrets.ps1"
Write-Host "  .\scripts\smoke_bybit_testnet.ps1"
