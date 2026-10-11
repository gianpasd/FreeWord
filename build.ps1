# Builds FreeWord with the Rust GNU toolchain.
# The windows-* crates need MinGW's dlltool/as on PATH; this adds the WinLibs
# install (winget: BrechtSanders.WinLibs.POSIX.UCRT) if it is present.
param([switch]$Debug, [switch]$Run)

$winlibs = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter "BrechtSanders.WinLibs*" -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($winlibs) {
    $env:PATH = (Join-Path $winlibs.FullName "mingw64\bin") + ";" + $env:PATH
}
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH

Set-Location $PSScriptRoot
$cmd = if ($Run) { "run" } else { "build" }
if ($Debug) {
    cargo $cmd
} else {
    cargo $cmd --release
}
