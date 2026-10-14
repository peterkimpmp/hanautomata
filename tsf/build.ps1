# Builds the Hanautomata TSF input method (S0 build) with Rust's GNU toolchain: no Visual Studio, no administrator.
#   .\build.ps1            x64 DLL + setup tool -> build\tsf\x64, then the setup tool's selftest (loads the DLL, no registration)
#   .\build.ps1 -Test      also the shared engine tests (core) and the Windows key-logic tests (tsf)
#   .\build.ps1 -X86       also the 32-bit DLL and setup tool for 32-bit applications (W-03) -> build\tsf\x86, and their
#                          selftest; uses the stable-i686-pc-windows-gnu toolchain when installed (its own 32-bit linker)
#   .\build.ps1 -X86 -Package  six-file payload + installer dependencies + SHA-256 -> deliverables\Hanautomata-tsf-s0-<commit>-<id>.zip
# Registration is a separate, deliberate step: build\tsf\x64\hanautomata-ime-setup.exe register (see README.md).
# CARGO_TARGET_DIR is honoured: point it at an ASCII path when the user folder has non-ASCII characters (MinGW linker).
# No dlltool is needed: the crates link through prebuilt import libraries (Cargo.toml explains the windows 0.59 pin).
param([switch]$Test, [switch]$X86, [switch]$Package)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = Split-Path $here
if ($Package -and -not $X86) { throw 'Installable packages require both architectures: use -X86 -Package.' }
. (Join-Path $here 'payload.ps1')
& (Join-Path $here 'build-options.ps1') -OutputDirectory (Join-Path $root 'build/tsf/ui')

function Invoke-Checked([string]$exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "$exe $($argv -join ' ') failed with exit code $LASTEXITCODE" }
}

function Test-Ascii([string]$text) { return $text -notmatch '[^\x00-\x7F]' }

Push-Location $here
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $here 'target' }
$temp, $tmp = $env:TEMP, $env:TMP
try {
    if (-not (Test-Ascii $target)) { Write-Warning "CARGO_TARGET_DIR $target is not ASCII; MinGW tools may fail. See README." }
    # MinGW ld cannot read gcc's response files under a non-ASCII TEMP (C:\Users\<Korean name>\...: "ld: cannot find
    # @...\cc*: Invalid argument", i686 link, 2026-10-12): use a folder inside the ASCII target folder for this build only.
    if (-not (Test-Ascii $env:TEMP) -and (Test-Ascii $target)) {
        $env:TEMP = $env:TMP = Join-Path $target 'tmp'
        New-Item -ItemType Directory -Force $env:TEMP | Out-Null
    }
    if ($Test) {
        Push-Location (Join-Path $root 'core')
        try { Invoke-Checked cargo @('test', '--release') } finally { Pop-Location }
        Invoke-Checked cargo @('test', '--release')
    }
    Invoke-Checked cargo @('build', '--release')
    $x64 = Join-Path $root 'build\tsf\x64'
    New-Item -ItemType Directory -Force $x64 | Out-Null
    Copy-Item (Join-Path $target 'release\hanautomata_tsf.dll'), (Join-Path $target 'release\hanautomata-ime-setup.exe') $x64 -Force
    Invoke-Checked (Join-Path $x64 'hanautomata-ime-setup.exe') @('selftest')
    if ($X86) {
        # The 32-bit setup tool too: its selftest loads the 32-bit DLL in a 32-bit process, as a 32-bit app would.
        $argv = @('build', '--release', '--target', 'i686-pc-windows-gnu')
        if (rustup toolchain list | Select-String -SimpleMatch 'stable-i686-pc-windows-gnu') { $argv = @('+stable-i686-pc-windows-gnu') + $argv }
        Invoke-Checked cargo $argv
        $built = Join-Path $target 'i686-pc-windows-gnu\release'
        $x86files = (Join-Path $built 'hanautomata_tsf.dll'), (Join-Path $built 'hanautomata-ime-setup.exe')
        # Test what cargo built. PowerShell 7 would not start the 32-bit tool inside a pipeline ("StandardOutputEncoding
        # is only supported when standard output is redirected", Windows 365, 2026-10-12); cmd starts it.
        cmd /c "`"$($x86files[1])`" selftest"
        if ($LASTEXITCODE -ne 0) { throw "x86 selftest failed with exit code $LASTEXITCODE" }
        $x86dir = Join-Path $root 'build\tsf\x86'
        New-Item -ItemType Directory -Force $x86dir | Out-Null
        Copy-Item $x86files $x86dir -Force -ErrorAction Stop
    }
    $hashes = [ordered]@{}
    foreach ($relative in Get-HanautomataPayloadFiles) {
        if (-not $X86 -and $relative.StartsWith('x86/')) { continue }
        $hashes[$relative] = (Get-FileHash -LiteralPath (Join-Path $root "build/tsf/$relative") -Algorithm SHA256).Hash
    }
    $hashes | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'build/tsf/sha256.json') -Encoding ascii
    if ($Package) {
        & (Join-Path $here 'package.ps1') -Source (Join-Path $root 'build/tsf')
    }
} finally {
    $env:TEMP, $env:TMP = $temp, $tmp
    Pop-Location
}
