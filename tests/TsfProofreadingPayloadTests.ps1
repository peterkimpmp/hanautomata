$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
. (Join-Path $root 'tsf/payload.ps1')
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('hanautomata-proof-payload-' + [Guid]::NewGuid().ToString('N'))
$source = Join-Path $testRoot 'source'
$count = 0
function Assert-That([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "Proofreading payload regression: $Message" }
    $script:count++
}
function Assert-Rejected([scriptblock]$Action, [string]$Message) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    Assert-That $rejected $Message
}
function Write-Manifest { $script:hashes | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $source 'sha256.json') -Encoding ascii }
try {
    $files = @(Get-HanautomataPayloadFiles)
    foreach ($arch in @('x64','x86')) {
        $files += "$arch/Hanautomata.Proofreading.exe"
        foreach ($name in @('hanautomata-proofread.exe','MANIFEST.json','LICENSE','THIRD-PARTY-NOTICES.md',
                'data/ko.aff','data/ko.dic','data/dictionary.lock.json','data/dictionary-source.zip',
                'data/LICENSE.md','data/LICENSE.GPL-3','sources/hunspell-sys.crate','libc++.dll','runtime-licenses/LICENSE.TXT')) {
            $files += "$arch/proofreading/$name"
        }
    }
    $hashes = [ordered]@{}
    foreach ($relative in $files) {
        $file = Join-Path $source $relative
        $null = New-Item -ItemType Directory -Force -Path (Split-Path $file)
        [IO.File]::WriteAllText($file, 'synthetic: ' + $relative)
        $hashes[$relative] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
    }
    Write-Manifest
    [IO.File]::WriteAllText((Join-Path $source 'x64/proofreading/private-unlisted.txt'), 'canary')
    $destination = Join-Path $testRoot 'staged'
    Copy-HanautomataPayload $source $destination
    foreach ($relative in $files) {
        Assert-That (Test-Path -LiteralPath (Join-Path $destination $relative) -PathType Leaf) "omitted $relative"
    }
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $destination 'x64/proofreading/private-unlisted.txt'))) 'unlisted file copied'
    $null = Assert-HanautomataPayload $destination
    Assert-That ((Get-ChildItem -LiteralPath $destination -Recurse -File).Count -eq ($files.Count + 1)) 'unexpected staged files'
    $stagedManifest = Get-Content -LiteralPath (Join-Path $destination 'sha256.json') -Raw | ConvertFrom-Json
    Assert-That (@($stagedManifest.PSObject.Properties).Count -eq $files.Count) 'staged manifest omitted extensions'
    Assert-Rejected { Copy-HanautomataPayload $source $destination } 'existing revision overwritten'

    $key = 'x86/Hanautomata.Proofreading.exe'; $saved = $hashes[$key]
    $hashes.Remove($key); Write-Manifest
    Assert-Rejected { Assert-HanautomataPayload $source } 'partial architecture accepted'
    $hashes[$key] = $saved; Write-Manifest
    $dictionary = Join-Path $source 'x64/proofreading/data/ko.aff'
    $savedBytes = [IO.File]::ReadAllBytes($dictionary)
    [IO.File]::WriteAllText($dictionary, 'tampered')
    $bad = Join-Path $testRoot 'bad'
    Assert-Rejected { Copy-HanautomataPayload $source $bad } 'dictionary tamper accepted'
    Assert-That (-not (Test-Path -LiteralPath $bad)) 'invalid data staged before validation'
    [IO.File]::WriteAllBytes($dictionary, $savedBytes)
    foreach ($invalid in @('x64/proofreading/../../escape.exe','x64/proofreading/data/file:stream',
            'x64/proofreading/data/CON','x64/proofreading/data/file.','x64/proofreading/data//file',
            'other/secret.txt')) {
        $hashes[$invalid] = $saved; Write-Manifest
        Assert-Rejected { Assert-HanautomataPayload $source } "unsafe manifest accepted: $invalid"
        $hashes.Remove($invalid)
    }
    Write-Manifest
    # Directory junctions must not let an elevated copy follow a source outside the package.
    $data = Join-Path $source 'x64/proofreading/data'
    $outside = Join-Path $testRoot 'outside'
    Move-Item -LiteralPath $data -Destination $outside
    $null = New-Item -ItemType Junction -Path $data -Target $outside
    try { Assert-Rejected { Assert-HanautomataPayload $source } 'directory junction accepted' }
    finally { [IO.Directory]::Delete($data) }
    Move-Item -LiteralPath $outside -Destination $data
    Write-Output "PASS: $count proofreading payload assertions; 0 failures"
} finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
