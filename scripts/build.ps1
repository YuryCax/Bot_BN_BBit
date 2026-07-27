# Build with MSVC environment (required on Windows for Rust)
# Usage: .\scripts\build.ps1
# Optional: .\scripts\build.ps1 -Extra " -p observer-bin -p executor-bin"

param(
    [string]$Extra = ""
)

$vcvars = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    Write-Error "MSVC Build Tools not found. Install: winget install Microsoft.VisualStudio.2022.BuildTools"
    exit 1
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:LOCALAPPDATA\Programs\Python\Python312;$env:LOCALAPPDATA\Programs\Python\Python312\Scripts;" + $env:Path
$root = Split-Path -Parent $PSScriptRoot
cmd /c "`"$vcvars`" && cd /d `"$root`" && cargo build --release$Extra"
exit $LASTEXITCODE
