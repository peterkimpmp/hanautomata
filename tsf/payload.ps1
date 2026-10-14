# Shared by packaging and both elevated installation paths. Importing this file has no side effects.
function Get-HanautomataPayloadFiles {
    @('x64/hanautomata-ime-setup.exe', 'x64/hanautomata_tsf.dll',
      'x86/hanautomata-ime-setup.exe', 'x86/hanautomata_tsf.dll',
      'ui/Hanautomata.ImeOptions.exe', 'ui/Hanautomata.ImeOptions.exe.config')
}
function Get-HanautomataRollbackTool([string]$Dll, [string]$Installation) {
    $root = [IO.Path]::GetFullPath($Installation).TrimEnd('\') + '\'
    $registered = (Resolve-Path -LiteralPath $Dll).Path
    if (-not $registered.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) { throw 'Previous runtime is outside the installation.' }
    $setup = Join-Path (Split-Path $registered) 'hanautomata-ime-setup.exe'
    if (-not (Test-Path -LiteralPath $setup -PathType Leaf) -or ((Get-Item -LiteralPath $setup -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Previous revision setup unavailable for rollback: $setup"
    }
    return $setup
}
function Assert-HanautomataPayload([string]$Source) {
    $manifest = Get-Content -LiteralPath (Join-Path $Source 'sha256.json') -Raw | ConvertFrom-Json
    foreach ($relative in Get-HanautomataPayloadFiles) {
        $expected = [string]$manifest.$relative
        $path = Join-Path $Source $relative
        if ($expected -notmatch '^[A-Fa-f0-9]{64}$' -or -not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing or invalid payload: $relative" }
        if (((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Reparse payload refused: $relative" }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $expected) { throw "Payload hash mismatch: $relative" }
    }
    return $manifest
}
function Copy-HanautomataPayload([string]$Source, [string]$Destination) {
    $manifest = Assert-HanautomataPayload $Source
    # Only a new destination is allowed: loaded executables and DLLs are never overwritten.
    if (Test-Path -LiteralPath $Destination) { throw "Payload destination already exists: $Destination" }
    $null = New-Item -ItemType Directory -Path $Destination
    foreach ($folder in @('x64', 'x86', 'ui')) { $null = New-Item -ItemType Directory -Path (Join-Path $Destination $folder) }
    foreach ($relative in Get-HanautomataPayloadFiles) {
        $targetFile = Join-Path $Destination $relative
        Copy-Item -LiteralPath (Join-Path $Source $relative) -Destination $targetFile
        if ((Get-FileHash -LiteralPath $targetFile -Algorithm SHA256).Hash -ne [string]$manifest.$relative) { throw "Staged hash mismatch: $relative" }
    }
    $verified = [ordered]@{}
    foreach ($relative in Get-HanautomataPayloadFiles) { $verified[$relative] = [string]$manifest.$relative }
    $verified | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Destination 'sha256.json') -Encoding ascii
}
