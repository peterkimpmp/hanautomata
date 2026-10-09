param([string]$InputFile, [string]$CorrectionsFile, [string]$OutputPrefix, [string]$CorePath, [string]$EnglishPath, [string]$KoreanLmPath, [string]$EnglishLmPath)
$ErrorActionPreference = 'Stop'
if (-not $InputFile) { throw 'Specify -InputFile <your own text sample>. The development sample is private and not distributed.' }
if (-not $CorrectionsFile) { $CorrectionsFile = [System.IO.Path]::GetTempFileName() }   # optional: raw<TAB>intended rows
if (-not $OutputPrefix) { $OutputPrefix = Join-Path $PSScriptRoot 'build/corpus-after' }
if (-not $CorePath) { $CorePath = Join-Path $PSScriptRoot 'src/Core.cs' }
if (-not $EnglishPath) { $EnglishPath = Join-Path $PSScriptRoot 'data/english.txt' }
if (-not $KoreanLmPath) { $KoreanLmPath = Join-Path $PSScriptRoot 'data/korean-lm.txt.gz' }
if (-not $EnglishLmPath) { $EnglishLmPath = Join-Path $PSScriptRoot 'data/english-lm.txt' }
$null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutputPrefix)
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$probe = $OutputPrefix + '.exe'
& $compiler /nologo /utf8output /target:exe /reference:System.Web.Extensions.dll "/out:$probe" "/resource:$EnglishPath,HanFlow.English.txt" "/resource:$KoreanLmPath,HanFlow.KoreanLM.txt.gz" "/resource:$EnglishLmPath,HanFlow.EnglishLM.txt" $CorePath (Join-Path $PSScriptRoot 'tests/CorpusProbe.cs')
if ($LASTEXITCODE -ne 0) { throw 'Corpus probe compilation failed.' }
& $probe $InputFile $CorrectionsFile $OutputPrefix
if ($LASTEXITCODE -ne 0) { throw 'Corpus evaluation failed.' }
Write-Output 'The JSON reports mismatches; a successful evaluator run does not mean every item was correct.'
