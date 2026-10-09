$ErrorActionPreference = 'Stop'
$packageRoot = Join-Path $PSScriptRoot 'deliverables'
$bundle = Join-Path $packageRoot 'Hanautomata-0.4.0'
$null = New-Item -ItemType Directory -Force -Path $bundle
foreach ($file in @('Hanautomata.exe', 'Hanautomata.exe.config')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot "build/$file") -Destination (Join-Path $bundle $file) -Force
}
foreach ($file in @('README.md', 'CHANGELOG.md', 'LICENSE', 'NOTICE.md', 'SECURITY.md', 'install.ps1')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination (Join-Path $bundle $file) -Force
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs') -Destination $bundle -Recurse -Force
$hashes = Get-ChildItem -LiteralPath $bundle -File -Recurse | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object FullName | ForEach-Object { '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash, $_.FullName.Substring($bundle.Length + 1).Replace('\','/') }
$hashes | Set-Content -LiteralPath (Join-Path $bundle 'SHA256SUMS.txt') -Encoding ASCII
Compress-Archive -LiteralPath $bundle -DestinationPath (Join-Path $packageRoot 'Hanautomata-0.4.0-win.zip') -Force
Get-ChildItem -LiteralPath $packageRoot -File | Select-Object Name,Length
