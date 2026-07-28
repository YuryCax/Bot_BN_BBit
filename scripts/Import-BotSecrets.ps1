# Load gitignored secrets into current PowerShell process.
# Usage:
#   .\scripts\Import-BotSecrets.ps1
#   .\scripts\Import-BotSecrets.ps1 -Path .\secrets.env

param(
    [string]$Path = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not $Path) {
    $candidates = @(
        (Join-Path $root "secrets.env"),
        (Join-Path $root ".env")
    )
    $Path = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Path -or -not (Test-Path $Path)) {
    Write-Error "No secrets file. Copy secrets.env.example -> secrets.env and fill keys."
}

Get-Content $Path | ForEach-Object {
    $line = $_.Trim()
    if (-not $line -or $line.StartsWith("#")) { return }
    $i = $line.IndexOf("=")
    if ($i -lt 1) { return }
    $name = $line.Substring(0, $i).Trim()
    $val = $line.Substring($i + 1).Trim()
    if ($val.StartsWith('"') -and $val.EndsWith('"')) {
        $val = $val.Substring(1, $val.Length - 2)
    }
    [Environment]::SetEnvironmentVariable($name, $val, "Process")
}

Write-Host "Loaded secrets from $Path (process env only)"
if ($env:BYBIT_TESTNET -eq "1" -and -not $env:BYBIT_WS_URL) {
    $env:BYBIT_WS_URL = "wss://stream-testnet.bybit.com/v5/public/linear"
    Write-Host "BYBIT_WS_URL defaulted for testnet"
}
