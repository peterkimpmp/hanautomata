param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'build/windows-quality/personal-assist-layout' }
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
foreach ($layoutMode in @('native', 'dpi96')) {
    # Native follows the desktop scale (200% on high-DPI machines); dpi96 keeps 96-DPI metrics. Both stay hidden.
    $layoutTests = Join-Path $OutputDirectory "Hanautomata.PersonalAssist.LayoutTests.$layoutMode.exe"
    $manifestArgument = if ($layoutMode -eq 'native') { "/win32manifest:$root/app.manifest" } else { '/nowin32manifest' }
    & $compiler /nologo /utf8output /target:exe /main:PersonalAssistLayoutTests "/out:$layoutTests" $manifestArgument /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Core.dll /reference:System.Web.Extensions.dll (Join-Path $root 'src/PersonalAssistClient.cs') (Join-Path $root 'src/PersonalAssistWindow.cs') (Join-Path $PSScriptRoot 'PersonalAssistLayoutTests.cs')
    if ($LASTEXITCODE -ne 0) { throw "Personal assist layout test compilation failed ($layoutMode)." }
    Write-Output "Layout metrics mode: $layoutMode"
    & $layoutTests
    if ($LASTEXITCODE -ne 0) { throw "Personal assist layout tests failed ($layoutMode)." }
}
