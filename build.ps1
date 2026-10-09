param([switch]$Test, [switch]$Integration, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$appRoot = $PSScriptRoot
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $appRoot 'build' }
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$framework = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319'
$compiler = Join-Path $framework 'csc.exe'
if (-not (Test-Path -LiteralPath $compiler)) { throw '.NET Framework 4.x compiler was not found.' }
$core = Join-Path $appRoot 'src/Core.cs'
$lexicon = Join-Path $appRoot 'data/english.txt'
$koreanLm = Join-Path $appRoot 'data/korean-lm.txt.gz'
$englishLm = Join-Path $appRoot 'data/english-lm.txt'
$koreanWords = Join-Path $appRoot 'data/korean-words.txt.gz'
$englishWords = Join-Path $appRoot 'data/english-words.txt.gz'
foreach ($required in @($lexicon, $koreanLm, $englishLm, $koreanWords, $englishWords)) { if (-not (Test-Path -LiteralPath $required)) { throw "Model resource missing: $required" } }
# Embedded resources: the English lexicon, the Korean syllable and English letter statistics (aggregate counts only) and the two band-only word lists.
$modelResources = @("/resource:$lexicon,Hanautomata.English.txt", "/resource:$koreanLm,Hanautomata.KoreanLM.txt.gz", "/resource:$englishLm,Hanautomata.EnglishLM.txt", "/resource:$koreanWords,Hanautomata.KoreanWords.txt.gz", "/resource:$englishWords,Hanautomata.EnglishWords.txt.gz")
$testResources = @(
    ("/resource:" + (Join-Path $appRoot 'tests/corpora/scorer-fixtures.tsv') + ",Hanautomata.ScorerFixtures.tsv"),
    ("/resource:" + (Join-Path $appRoot 'tests/corpora/heldout-korean-1000.tsv') + ",Hanautomata.HeldoutKorean.tsv"),
    ("/resource:" + (Join-Path $appRoot 'tests/corpora/heldout-english-1000.tsv') + ",Hanautomata.HeldoutEnglish.tsv"))
if ($Test) {
    & $compiler /nologo /utf8output /target:exe /optimize+ /define:HANAUTOMATA_TESTS /reference:System.Web.Extensions.dll "/out:$OutputDirectory/Hanautomata.Tests.exe" @modelResources @testResources $core (Join-Path $appRoot 'src/PreferenceStore.cs') (Join-Path $appRoot 'tests/CoreTests.cs')
    if ($LASTEXITCODE -ne 0) { throw 'Test compilation failed.' }
    & (Join-Path $OutputDirectory 'Hanautomata.Tests.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Core tests failed.' }
}
if (-not $Test -or $Integration) {
    $sources = @(Get-ChildItem -LiteralPath (Join-Path $appRoot 'src') -Filter '*.cs' | ForEach-Object FullName)
    & $compiler /nologo /utf8output /target:winexe /platform:anycpu /optimize+ "/win32manifest:$appRoot/app.manifest" "/out:$OutputDirectory/Hanautomata.exe" /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Core.dll /reference:System.Web.Extensions.dll "/reference:$framework/WPF/UIAutomationClient.dll" "/reference:$framework/WPF/UIAutomationTypes.dll" "/reference:$framework/WPF/WindowsBase.dll" @modelResources @sources
    if ($LASTEXITCODE -ne 0) { throw 'Application compilation failed.' }
    Copy-Item -LiteralPath (Join-Path $appRoot 'app.config') -Destination (Join-Path $OutputDirectory 'Hanautomata.exe.config')
    Write-Output (Join-Path $OutputDirectory 'Hanautomata.exe')
}
if ($Integration) {
    $resultPath = Join-Path $OutputDirectory 'integration-results.json'
    $testProcess = Start-Process -FilePath (Join-Path $OutputDirectory 'Hanautomata.exe') -ArgumentList @('--integration-test', ('"' + $resultPath + '"')) -PassThru -WindowStyle Hidden
    if (-not $testProcess.WaitForExit(90000)) { $testProcess.Kill(); throw 'Integration test timed out.' }
    if ($testProcess.ExitCode -ne 0) { throw "Integration test failed; inspect $resultPath" }
    Get-Content -LiteralPath $resultPath
}
