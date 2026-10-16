param([string]$OutputDirectory = 'build/proofreading-windows')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
Push-Location $root
try {
    $null = New-Item -ItemType Directory -Force -Path $OutputDirectory
    $out = (Resolve-Path $OutputDirectory).Path
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    $contract = Join-Path $root 'src/ProofreadingContract.cs'
    $window = Join-Path $root 'src/ProofreadingWindow.cs'
    $preview = Join-Path $root 'tests/ProofreadingPreview.cs'
    & $compiler /nologo /utf8output /target:winexe /main:Hanautomata.ProofreadingProgram /platform:anycpu /optimize+ "/out:$out/Hanautomata.Proofreading.exe" /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Core.dll /reference:System.Web.Extensions.dll $contract $window
    if ($LASTEXITCODE -ne 0) { throw 'Review UI build failed' }
    & $compiler /nologo /utf8output /target:winexe /main:Hanautomata.ProofreadingPreview /platform:anycpu /optimize+ "/out:$out/Hanautomata.Proofreading.Preview.exe" /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Core.dll /reference:System.Web.Extensions.dll $contract $window $preview
    if ($LASTEXITCODE -ne 0) { throw 'Preview editor build failed' }
    cargo build --release --locked --manifest-path core/tools/proofread/Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw 'Native checker build failed (C++ compiler and libclang required)' }
    python core/tools/proofread/prepare_dictionary.py --with-source
    if ($LASTEXITCODE -ne 0) { throw 'Dictionary preparation failed' }
    $targetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'core/tools/proofread/target' }
    python core/tools/proofread/package.py --binary (Join-Path $targetDirectory 'release/hanautomata-proofread.exe') --data build/proofreading-data --output "$out/proofreading"
    if ($LASTEXITCODE -ne 0) { throw 'Payload validation failed' }
    Write-Host "Built experimental payload: $out (no installation or registration)"
} finally { Pop-Location }
