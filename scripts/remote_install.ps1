# One-shot: scp release tarball + install on remote Ubuntu host.
# Examples:
#   .\scripts\remote_install.ps1 -Role tokyo -SshHost ubuntu@1.2.3.4 -TarPath dist\bot-release-xxx.tar.gz
#   .\scripts\remote_install.ps1 -Role singapore -SshHost ubuntu@5.6.7.8 -PeerIp 10.0.1.10 -TarPath dist\bot-release-xxx.tar.gz -SecretsPath .\secrets.env

param(
    [Parameter(Mandatory = $true)][ValidateSet("tokyo", "singapore")][string]$Role,
    [Parameter(Mandatory = $true)][string]$SshHost,
    [Parameter(Mandatory = $true)][string]$TarPath,
    [string]$PeerIp = "",
    [string]$SecretsPath = "",
    [string]$SshKey = ""
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $TarPath)) { Write-Error "Tar not found: $TarPath" }

$sshArgs = @()
if ($SshKey) { $sshArgs += @("-i", $SshKey) }

$remoteTar = "/tmp/bot-release.tar.gz"
Write-Host "scp $TarPath -> ${SshHost}:$remoteTar"
& scp @sshArgs $TarPath "${SshHost}:$remoteTar"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if ($SecretsPath -and (Test-Path $SecretsPath)) {
    Write-Host "scp secrets -> ${SshHost}:/tmp/bot-secrets.env"
    & scp @sshArgs $SecretsPath "${SshHost}:/tmp/bot-secrets.env"
}

$peerExport = if ($PeerIp) { "export PEER_IP='$PeerIp'; " } else { "" }
$secretsCmd = @"
if [ -f /tmp/bot-secrets.env ]; then
  install -m 600 -o bot -g bot /tmp/bot-secrets.env /etc/bot/secrets.env || true
  rm -f /tmp/bot-secrets.env
fi
"@

$remote = @"
set -euo pipefail
sudo mkdir -p /tmp/bot-rel
sudo tar -xzf $remoteTar -C /tmp/bot-rel
cd /tmp/bot-rel
$peerExport
sudo ROLE=$Role bash install.sh
$secretsCmd
sudo systemctl restart observer 2>/dev/null || true
sudo systemctl restart executor control-panel telegram-alerts 2>/dev/null || true
echo REMOTE_INSTALL_OK role=$Role
"@

Write-Host "ssh install ROLE=$Role PEER_IP=$PeerIp"
& ssh @sshArgs $SshHost $remote
exit $LASTEXITCODE
