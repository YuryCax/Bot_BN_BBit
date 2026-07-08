# Build with MSVC environment (required on Windows for Rust)
$vcvars = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    Write-Error "MSVC Build Tools not found. Install: winget install Microsoft.VisualStudio.2022.BuildTools"
    exit 1
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:LOCALAPPDATA\Programs\Python\Python312;$env:LOCALAPPDATA\Programs\Python\Python312\Scripts;" + $env:Path
cmd /c "`"$vcvars`" && cd /d `"$PSScriptRoot\..`" && cargo build --release @args"
