param([string]$SourceDirectory)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $SourceDirectory) { $SourceDirectory = Join-Path $repo 'build' }
$buildRoot = [IO.Path]::GetFullPath((Join-Path $repo 'build'))
$testRoot = Join-Path $buildRoot ('release-test-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $testRoot
$count = 0
function Assert-Release([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "Release regression: $Message" }
    $script:count++
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
try {
    & (Join-Path $repo 'package.ps1') -SourceDirectory $SourceDirectory -OutputDirectory $testRoot | Out-Null
    $oldBundle = Join-Path $testRoot 'Hanautomata-0.4.0'
    $null = New-Item -ItemType Directory -Path $oldBundle
    Set-Content -LiteralPath (Join-Path $oldBundle 'stale-private-canary.txt') -Value 'Must never ship'
    & (Join-Path $repo 'package.ps1') -SourceDirectory $SourceDirectory -OutputDirectory $testRoot | Out-Null
    $zip = Join-Path $testRoot 'Hanautomata-0.4.0-win.zip'
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        Assert-Release (@($archive.Entries | Where-Object { $_.FullName -match 'stale-private-canary|HanFlow|\.pyc$' }).Count -eq 0) 'stale files or obsolete paths in ZIP'
        Assert-Release (@($archive.Entries | Where-Object { $_.FullName -match '/docs/docs/' }).Count -eq 0) 'nested documentation after repeat packaging'
    } finally { $archive.Dispose() }
    $unpacked = Join-Path $testRoot 'unpacked'
    Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
    $portable = Join-Path $unpacked 'Hanautomata-0.4.0'
    & (Join-Path $portable 'install.ps1') -ValidateOnly | Out-Null
    Assert-Release $true 'portable installer resolves its executable without -SourceDirectory'
    foreach ($line in Get-Content -LiteralPath (Join-Path $portable 'SHA256SUMS.txt')) {
        $parts = $line -split '  ',2
        Assert-Release ((Get-FileHash -LiteralPath (Join-Path $portable $parts[1]) -Algorithm SHA256).Hash -eq $parts[0]) ('bundle checksum: ' + $parts[1])
    }
    $sum = (Get-Content -LiteralPath (Join-Path $testRoot 'SHA256SUMS.txt')) -split '  ',2
    Assert-Release ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -eq $sum[0]) 'published ZIP checksum'
    $manifest = Get-Content -LiteralPath (Join-Path $testRoot 'release-manifest.json') -Raw | ConvertFrom-Json
    Assert-Release ((Get-FileHash -LiteralPath (Join-Path $portable 'Hanautomata.exe') -Algorithm SHA256).Hash -eq $manifest.executable_sha256) 'packaging preserves the tested executable'
    $config = Join-Path $portable 'Hanautomata.exe.config'
    Remove-Item -LiteralPath $config
    $rejected = $false
    try { & (Join-Path $portable 'install.ps1') -ValidateOnly | Out-Null } catch { $rejected = $_.Exception.Message -match 'config is required' }
    Assert-Release $rejected 'incomplete installation must fail before modifying the machine'
    Write-Output "PASS: $count release assertions; 0 failures"
} finally {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $buildRoot -or [IO.Path]::GetFileName($resolved) -notmatch '^release-test-[a-f0-9]{32}$') { throw 'Unsafe test cleanup path' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
