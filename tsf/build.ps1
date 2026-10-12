# Builds the Hanautomata TSF input method (S0 build) with Rust's GNU toolchain: no Visual Studio, no administrator.
#   .\build.ps1            x64 DLL + setup tool -> build\tsf\x64, then the setup tool's selftest (loads the DLL, no registration)
#   .\build.ps1 -Test      also the shared engine tests (core) and the Windows key-logic tests (tsf)
#   .\build.ps1 -X86       also the 32-bit DLL for 32-bit applications (W-03) -> build\tsf\x86; uses the
#                          stable-i686-pc-windows-gnu toolchain when installed (it carries its own 32-bit linker)
#   .\build.ps1 -Package   zip + SHA-256 -> deliverables\Hanautomata-tsf-s0-<commit>.zip
# Registration is a separate, deliberate step: build\tsf\x64\hanautomata-ime-setup.exe register (see README.md).
# CARGO_TARGET_DIR is honoured: point it at an ASCII path when the user folder has non-ASCII characters (MinGW linker).
param([switch]$Test, [switch]$X86, [switch]$Package)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = Split-Path $here

function Invoke-Checked([string]$exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "$exe $($argv -join ' ') failed with exit code $LASTEXITCODE" }
}

# The GNU toolchain carries its own MinGW tools (gcc, ld, dlltool) in bin\self-contained, but rustc looks for dlltool
# (raw-dylib imports of the windows crate) on PATH. Put the toolchain's own folder first.
function Use-SelfContained([string]$toolchain, [string]$triple) {
    $sysroot = if ($toolchain) { (& rustc "+$toolchain" --print sysroot) } else { (& rustc --print sysroot) }
    $dir = Join-Path $sysroot.Trim() "lib\rustlib\$triple\bin\self-contained"
    if (Test-Path (Join-Path $dir 'dlltool.exe')) { $env:Path = "$dir;$env:Path" }
}

function Test-Ascii([string]$text) { return $text -notmatch '[^\x00-\x7F]' }

Push-Location $here
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $here 'target' }
$path, $temp, $tmp = $env:Path, $env:TEMP, $env:TMP
try {
    Use-SelfContained '' 'x86_64-pc-windows-gnu'
    # MinGW dlltool cannot create its temporary files under a non-ASCII folder (C:\Users\<Korean name>\...): use one
    # inside the ASCII target folder for this build only.
    if (-not (Test-Ascii $target)) { Write-Warning "CARGO_TARGET_DIR $target is not ASCII; MinGW tools may fail. See README." }
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
    if ($X86) {
        $argv = @('build', '--release', '--lib', '--target', 'i686-pc-windows-gnu')
        $x64path = $env:Path
        if (rustup toolchain list | Select-String -SimpleMatch 'stable-i686-pc-windows-gnu') {
            $argv = @('+stable-i686-pc-windows-gnu') + $argv
            Use-SelfContained 'stable-i686-pc-windows-gnu' 'i686-pc-windows-gnu'
        }
        try { Invoke-Checked cargo $argv } finally { $env:Path = $x64path }
        $x86dir = Join-Path $root 'build\tsf\x86'
        New-Item -ItemType Directory -Force $x86dir | Out-Null
        Copy-Item (Join-Path $target 'i686-pc-windows-gnu\release\hanautomata_tsf.dll') $x86dir -Force
    }
    Invoke-Checked (Join-Path $x64 'hanautomata-ime-setup.exe') @('selftest')
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
    $env:Path, $env:TEMP, $env:TMP = $path, $temp, $tmp
    Pop-Location
}
