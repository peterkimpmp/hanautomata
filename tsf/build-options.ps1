param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$exe = Join-Path $OutputDirectory 'Hanautomata.ImeOptions.exe'
& $compiler /nologo /utf8output /target:winexe /main:Hanautomata.ImeOptionsProgram /platform:anycpu /optimize+ "/win32manifest:$root/app.manifest" "/out:$exe" /reference:System.Windows.Forms.dll /reference:System.Drawing.dll /reference:System.Core.dll /reference:System.Web.Extensions.dll /reference:System.Runtime.Serialization.dll /reference:System.Xml.Linq.dll (Join-Path $root 'src/ImeSettings.cs') (Join-Path $root 'src/ImeSharedAccess.cs') (Join-Path $root 'src/ImeOptions.cs') (Join-Path $root 'src/ImeLearning.cs') (Join-Path $root 'src/AssemblyInfo.cs')
if ($LASTEXITCODE -ne 0) { throw 'IME options compilation failed.' }
Copy-Item -LiteralPath (Join-Path $root 'app.config') -Destination ($exe + '.config')
