param([string]$Node = 'node')
$ErrorActionPreference = 'Stop'
$cache = Join-Path $PSScriptRoot 'build/references'
$null = New-Item -ItemType Directory -Force -Path $cache
$references = @(
    @{ Name='hangul.cjs'; Url='https://raw.githubusercontent.com/e-/Hangul.js/325f7237a030741a10cbcbc2d9b8fac770d9e592/hangul.js'; Hash='FCCED0F7C24E9CC936D68ECA55886B7DC9B48C705416E0C6F29FDB86179BA4AF' },
    @{ Name='inko.cjs'; Url='https://raw.githubusercontent.com/738/inko/6bcb04b075f282b945dfcdecdf943d21fbc8a5ab/index.js'; Hash='3B03704F6A57293E46D0DC5D93AB7E46126632E7BB7135C589CA1E234F031D8B' }
)
foreach ($reference in $references) {
    $file = Join-Path $cache $reference.Name
    if (-not (Test-Path -LiteralPath $file)) { Invoke-WebRequest -Uri $reference.Url -OutFile $file }
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $reference.Hash) { throw "Pinned reference hash mismatch: $($reference.Name)" }
}
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$probe = Join-Path $cache 'ReferenceProbe.exe'
& $compiler /nologo /utf8output /target:exe /optimize+ "/out:$probe" (Join-Path $PSScriptRoot 'src/Core.cs') (Join-Path $PSScriptRoot 'tests/ReferenceProbe.cs')
if ($LASTEXITCODE -ne 0) { throw 'Reference probe compilation failed.' }
& $Node (Join-Path $PSScriptRoot 'tests/reference-differential.cjs') $probe $cache (Join-Path $PSScriptRoot 'build/reference-results.json')
if ($LASTEXITCODE -ne 0) { throw 'Reference comparison failed; inspect build/reference-results.json.' }
