param([string]$SourceDirectory, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
if (-not $SourceDirectory) { $SourceDirectory = Join-Path $PSScriptRoot 'build' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $PSScriptRoot 'deliverables' }
foreach ($file in @('Hanautomata.exe', 'Hanautomata.exe.config')) {
    if (-not (Test-Path -LiteralPath (Join-Path $SourceDirectory $file) -PathType Leaf)) { throw "Release input missing: $file" }
}
$exe = Join-Path $SourceDirectory 'Hanautomata.exe'
$fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo((Resolve-Path -LiteralPath $exe).Path).FileVersion
if ($fileVersion -ne '0.4.0.0') { throw "Expected 0.4.0.0; got $fileVersion" }
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$packageRoot = (Resolve-Path -LiteralPath $OutputDirectory).Path
$staging = Join-Path $packageRoot ('.staging-' + [Guid]::NewGuid().ToString('N'))
$bundle = Join-Path $staging 'Hanautomata-0.4.0'
$zip = Join-Path $packageRoot 'Hanautomata-0.4.0-win.zip'
$sourceCommit = $null
$sourceDirty = $null
if (Get-Command git -ErrorAction SilentlyContinue) {
    $gitRoot = & git -C $PSScriptRoot rev-parse --show-toplevel 2>$null
    if ($LASTEXITCODE -eq 0 -and [IO.Path]::GetFullPath($gitRoot) -eq [IO.Path]::GetFullPath($PSScriptRoot)) {
        $sourceCommit = & git -C $PSScriptRoot rev-parse HEAD
        $sourceDirty = [bool](& git -C $PSScriptRoot status --porcelain --untracked-files=no)
    }
}
try {
    $null = New-Item -ItemType Directory -Path $bundle
    foreach ($file in @('Hanautomata.exe', 'Hanautomata.exe.config')) {
        Copy-Item -LiteralPath (Join-Path $SourceDirectory $file) -Destination (Join-Path $bundle $file)
    }
    foreach ($file in @('README.md', 'CHANGELOG.md', 'LICENSE', 'NOTICE.md', 'SECURITY.md', 'install.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination (Join-Path $bundle $file)
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs') -Destination (Join-Path $bundle 'docs') -Recurse
    $hashes = Get-ChildItem -LiteralPath $bundle -File -Recurse | Sort-Object FullName | ForEach-Object {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash, $_.FullName.Substring($bundle.Length + 1).Replace('\','/')
    }
    $hashes | Set-Content -LiteralPath (Join-Path $bundle 'SHA256SUMS.txt') -Encoding ASCII
    Compress-Archive -LiteralPath $bundle -DestinationPath $zip -Force
    ('{0}  {1}' -f (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash, [IO.Path]::GetFileName($zip)) |
        Set-Content -LiteralPath (Join-Path $packageRoot 'SHA256SUMS.txt') -Encoding ASCII
    [ordered]@{ version = $fileVersion; source_commit = $sourceCommit; source_dirty = $sourceDirty;
        executable_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;
        archive = [IO.Path]::GetFileName($zip); archive_sha256 = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $packageRoot 'release-manifest.json') -Encoding UTF8
} finally {
    # Only this invocation's staging directory may be removed; never reuse old package contents.
    $resolved = [IO.Path]::GetFullPath($staging)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $packageRoot -or [IO.Path]::GetFileName($resolved) -notmatch '^\.staging-[a-f0-9]{32}$') { throw 'Unsafe staging cleanup path' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
Get-Item -LiteralPath $zip, (Join-Path $packageRoot 'SHA256SUMS.txt'), (Join-Path $packageRoot 'release-manifest.json') | Select-Object Name,Length
