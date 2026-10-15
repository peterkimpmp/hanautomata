param(
    [string]$Source = (Join-Path (Split-Path $PSScriptRoot) 'build/tsf'),
    [string]$OutputDirectory = (Join-Path (Split-Path $PSScriptRoot) 'deliverables')
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'payload.ps1')
$root = Split-Path $PSScriptRoot
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$outputRoot = (Resolve-Path -LiteralPath $OutputDirectory).Path
$id = [Guid]::NewGuid().ToString('N')
$stage = Join-Path $outputRoot ('.tsf-package-' + $id)
$commit = (git -C $root rev-parse --short HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit.' }
$version = (Get-Content -LiteralPath (Join-Path $root 'VERSION') -Raw).Trim()
$zip = Join-Path $outputRoot "Hanautomata-ime-$version-win.zip"
try {
    Copy-HanautomataPayload $Source $stage
    foreach ($name in @('install-machine.ps1','update-machine.ps1','payload.ps1','README.md')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $stage $name)
    }
    [ordered]@{ version = $version; source_commit = $commit } | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $stage 'release-manifest.json') -Encoding UTF8
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
    "$hash  $(Split-Path $zip -Leaf)" | Set-Content -LiteralPath ($zip + '.sha256') -Encoding ascii
    Write-Output $zip
} finally {
    $resolved = [IO.Path]::GetFullPath($stage)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $outputRoot -or [IO.Path]::GetFileName($resolved) -ne ('.tsf-package-' + $id)) { throw 'Unsafe package cleanup path.' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
