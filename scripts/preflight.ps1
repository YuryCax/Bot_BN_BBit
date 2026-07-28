# Production preflight: unit tests + config/edge gates (no exchange orders).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

$vcvars = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    Write-Error "MSVC Build Tools not found"
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:LOCALAPPDATA\Programs\Python\Python312;$env:LOCALAPPDATA\Programs\Python\Python312\Scripts;" + $env:Path

Write-Host "== cargo test =="
cmd /c "`"$vcvars`" && cd /d `"$root`" && cargo test --workspace --all-targets --quiet"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "== config/edge gate =="
python -c @"
import tomllib
from pathlib import Path
cfg = tomllib.loads(Path('config/config.toml').read_bytes())
mode = cfg['deployment']['mode'].lower()
assert mode in ('dev', 'paper', 'live'), mode
edge = tomllib.loads(Path(cfg['deployment']['edge_profile_path']).read_bytes())
meta = edge['meta']
print(f\"mode={mode} edge_status={meta.get('status')} data_source={meta.get('data_source')} days={meta.get('research_period_days')}\")
if mode in ('paper', 'live'):
    assert meta.get('status') == 'pass'
    assert meta.get('data_source') in ('live', 'binance_vision')
    assert int(meta.get('research_period_days', 0)) >= 14
    print('edge gate OK for paper/live')
else:
    print('dev mode: edge gate skipped until LiveDownload pass')
"@
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (Test-Path "secrets.env") { Write-Host "secrets.env present" } else { Write-Host "WARN: no secrets.env" }
Write-Host "PREFLIGHT_OK"
exit 0
