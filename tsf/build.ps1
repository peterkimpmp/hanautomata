# Builds the Hanautomata TSF input method (S0 build) with Rust's GNU toolchain: no Visual Studio, no administrator.
#   .\build.ps1            x64 DLL + setup tool -> build\tsf\x64, then the setup tool's selftest (loads the DLL, no registration)
#   .\build.ps1 -Test      also the shared engine tests (core) and the Windows key-logic tests (tsf)
#   .\build.ps1 -X86       also the 32-bit DLL and setup tool for 32-bit applications (W-03) -> build\tsf\x86, and their
#                          selftest; uses the stable-i686-pc-windows-gnu toolchain when installed (its own 32-bit linker)
#   .\build.ps1 -Package   zip + SHA-256 -> deliverables\Hanautomata-tsf-s0-<commit>.zip
# Registration is a separate, deliberate step: build\tsf\x64\hanautomata-ime-setup.exe register (see README.md).
# CARGO_TARGET_DIR is honoured: point it at an ASCII path when the user folder has non-ASCII characters (MinGW linker).
# No dlltool is needed: the crates link through prebuilt import libraries (Cargo.toml explains the windows 0.59 pin).
param([switch]$Test, [switch]$X86, [switch]$Package)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = Split-Path $here

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
        $x86dir = Join-Path $root 'build\tsf\x86'
        New-Item -ItemType Directory -Force $x86dir | Out-Null
        $built = Join-Path $target 'i686-pc-windows-gnu\release'
        Copy-Item (Join-Path $built 'hanautomata_tsf.dll'), (Join-Path $built 'hanautomata-ime-setup.exe') $x86dir -Force
        Invoke-Checked (Join-Path $x86dir 'hanautomata-ime-setup.exe') @('selftest')
    }
    if ($Package) {
        $commit = (git -C $root rev-parse --short HEAD).Trim()
        $dist = Join-Path $root 'deliverables'
        New-Item -ItemType Directory -Force $dist | Out-Null
        $zip = Join-Path $dist "Hanautomata-tsf-s0-$commit.zip"
        $stage = Join-Path $root 'build\tsf\package'
        Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force $stage | Out-Null
        Copy-Item $x64 (Join-Path $stage 'x64') -Recurse
        if ($X86) { Copy-Item (Join-Path $root 'build\tsf\x86') (Join-Path $stage 'x86') -Recurse }
        Copy-Item 'README.md' $stage
        Remove-Item $zip -Force -ErrorAction SilentlyContinue
        Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
        $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
        "$hash  $(Split-Path $zip -Leaf)" | Set-Content "$zip.sha256" -Encoding ascii
        Write-Output "packaged $zip ($((Get-Item $zip).Length) bytes) SHA256 $hash"
    }
} finally {
    $env:TEMP, $env:TMP = $temp, $tmp
    Pop-Location
}
