# Authenticated Binance read-only connectivity smoke.
# Calls only GET /api/v3/account and never sends an order.

param(
    [string]$SecretsPath = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

if ($SecretsPath) {
    & "$root\scripts\Import-BotSecrets.ps1" -Path $SecretsPath
} elseif (Test-Path (Join-Path $root "secrets.env")) {
    & "$root\scripts\Import-BotSecrets.ps1"
}

if (-not $env:BINANCE_API_KEY -or -not $env:BINANCE_API_SECRET) {
    Write-Error "BINANCE_API_KEY/BINANCE_API_SECRET are empty. Add read-only keys to secrets.env."
}

$base = if ($env:BINANCE_API_BASE) {
    $env:BINANCE_API_BASE.TrimEnd("/")
} else {
    "https://api.binance.com"
}
$timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$query = "omitZeroBalances=true&recvWindow=5000&timestamp=$timestamp"
$hmac = [System.Security.Cryptography.HMACSHA256]::new(
    [Text.Encoding]::UTF8.GetBytes($env:BINANCE_API_SECRET)
)
try {
    $signature = -join ($hmac.ComputeHash([Text.Encoding]::UTF8.GetBytes($query)) |
        ForEach-Object { $_.ToString("x2") })
} finally {
    $hmac.Dispose()
}

$headers = @{ "X-MBX-APIKEY" = $env:BINANCE_API_KEY }
$account = Invoke-RestMethod -Method Get `
    -Uri "$base/api/v3/account?$query&signature=$signature" `
    -Headers $headers

if ($null -eq $account.updateTime -or $null -eq $account.permissions) {
    Write-Error "Binance returned an unexpected account response."
}

Write-Host "BINANCE READ-ONLY SMOKE OK: authenticated account read succeeded; no order endpoint called."
Write-Host "Account permissions reported: $($account.permissions -join ', ')"
