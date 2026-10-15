param([string]$OutputDirectory, [string]$NativeSetup, [string]$NativeFixture)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'build/windows-quality/ime-learning-tests' }
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$exe = Join-Path $OutputDirectory 'Hanautomata.ImeLearning.Tests.exe'
& $compiler /nologo /utf8output /target:exe /main:ImeLearningTests "/out:$exe" /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Core.dll /reference:System.Web.Extensions.dll /reference:System.Runtime.Serialization.dll /reference:System.Xml.Linq.dll (Join-Path $root 'src/ImeSettings.cs') (Join-Path $root 'src/ImeSharedAccess.cs') (Join-Path $root 'src/ImeLearning.cs') (Join-Path $PSScriptRoot 'ImeLearningTests.cs')
if ($LASTEXITCODE -ne 0) { throw 'Learning management test compilation failed.' }
Copy-Item -LiteralPath (Join-Path $root 'app.config') -Destination ($exe + '.config')
if ($NativeFixture -and -not $NativeSetup) { throw 'NativeFixture requires NativeSetup.' }
if ($NativeFixture) { & $exe $NativeSetup $NativeFixture } elseif ($NativeSetup) { & $exe $NativeSetup } else { & $exe }
if ($LASTEXITCODE -ne 0) { throw 'Learning management tests failed.' }
