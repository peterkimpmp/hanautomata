param([string]$SourceDirectory)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $SourceDirectory) { $SourceDirectory = Join-Path $repo 'build' }
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$testExe = Join-Path $SourceDirectory 'Hanautomata.StartupTests.exe'
& $compiler /nologo /utf8output /target:exe /reference:Microsoft.CSharp.dll /reference:System.Core.dll /reference:System.Web.Extensions.dll "/out:$testExe" (Join-Path $repo 'src/StartupRegistration.cs') (Join-Path $PSScriptRoot 'StartupTests.cs')
if ($LASTEXITCODE -ne 0) { throw 'Startup test compilation failed.' }
& $testExe (Join-Path $SourceDirectory 'Hanautomata.exe')
if ($LASTEXITCODE -ne 0) { throw 'Startup tests failed.' }
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'StartupRegression.ps1') -SourceDirectory $SourceDirectory
if ($LASTEXITCODE -ne 0) { throw 'Legacy startup regression failed.' }
