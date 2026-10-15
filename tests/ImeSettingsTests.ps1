param([string]$OutputDirectory, [switch]$BuildPreview)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'build/windows-quality/ime-settings-tests' }
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$exe = Join-Path $OutputDirectory 'Hanautomata.ImeSettings.Tests.exe'
& $compiler /nologo /utf8output /target:exe /optimize+ /reference:System.Web.Extensions.dll /reference:System.Runtime.Serialization.dll /reference:System.Xml.Linq.dll "/out:$exe" (Join-Path $root 'src/ImeSettings.cs') (Join-Path $root 'src/ImeSharedAccess.cs') (Join-Path $PSScriptRoot 'ImeSettingsTests.cs')
if ($LASTEXITCODE -ne 0) { throw 'IME settings test compilation failed.' }
& $exe (Join-Path $PSScriptRoot 'fixtures/tsf-settings.json') (Join-Path $OutputDirectory 'csharp-settings.json')
if ($LASTEXITCODE -ne 0) { throw 'IME settings tests failed.' }
foreach ($layoutMode in @('native', 'dpi96')) {
    # Also exercise 96-DPI metrics on a high-DPI developer desktop. Both fixtures remain hidden.
    $layoutTests = Join-Path $OutputDirectory "Hanautomata.ImeOptions.LayoutTests.$layoutMode.exe"
    $manifestArgument = if ($layoutMode -eq 'native') { "/win32manifest:$root/app.manifest" } else { '/nowin32manifest' }
    & $compiler /nologo /utf8output /target:exe /main:ImeOptionsLayoutTests "/out:$layoutTests" $manifestArgument /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll /reference:System.Runtime.Serialization.dll /reference:System.Xml.Linq.dll (Join-Path $root 'src/ImeSettings.cs') (Join-Path $root 'src/ImeSharedAccess.cs') (Join-Path $root 'src/ImeOptions.cs') (Join-Path $root 'src/ImeLearning.cs') (Join-Path $PSScriptRoot 'ImeOptionsLayoutTests.cs')
    if ($LASTEXITCODE -ne 0) { throw "IME layout test compilation failed ($layoutMode)." }
    Write-Output "Layout metrics mode: $layoutMode"
    & $layoutTests
    if ($LASTEXITCODE -ne 0) { throw "IME layout tests failed ($layoutMode)." }
}
if ($BuildPreview) {
    $preview = Join-Path $OutputDirectory 'Hanautomata.ImeOptions.Preview.exe'
    & $compiler /nologo /utf8output /target:winexe /main:ImeOptionsPreview "/out:$preview" "/win32manifest:$root/app.manifest" /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Web.Extensions.dll /reference:System.Runtime.Serialization.dll /reference:System.Xml.Linq.dll (Join-Path $root 'src/ImeSettings.cs') (Join-Path $root 'src/ImeSharedAccess.cs') (Join-Path $root 'src/ImeOptions.cs') (Join-Path $root 'src/ImeLearning.cs') (Join-Path $PSScriptRoot 'ImeOptionsPreview.cs')
    if ($LASTEXITCODE -ne 0) { throw 'IME options preview compilation failed.' }
    Copy-Item -LiteralPath (Join-Path $root 'app.config') -Destination ($preview + '.config')
    Write-Output $preview
}
