# Shared by packaging and both elevated installation paths. Importing this file has no side effects.
function Get-HanautomataPayloadFiles([object]$Manifest = $null) {
    $base = @('x64/hanautomata-ime-setup.exe', 'x64/hanautomata_tsf.dll',
      'x86/hanautomata-ime-setup.exe', 'x86/hanautomata_tsf.dll',
      'ui/Hanautomata.ImeOptions.exe', 'ui/Hanautomata.ImeOptions.exe.config')
    $extra = @()
    if ($null -ne $Manifest) {
        foreach ($property in $Manifest.PSObject.Properties) {
            $name = $property.Name
            if ($base -ccontains $name) { continue }
            if ($name -cnotmatch '^(x64|x86)/(Hanautomata\.Proofreading\.exe(\.config)?|proofreading/.+)$') {
                throw "Unsupported payload path: $name"
            }
            foreach ($part in $name.Split('/')) {
                if ($part -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._+-]*$' -or $part.EndsWith('.') -or
                    $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') { throw "Unsafe payload path: $name" }
            }
            $extra += $name
        }
        if ($extra.Count -gt 0) {
            # A feature payload must be complete for both COM architectures. The external checker
            # can be x64 for both; it is never loaded into the x86 host process.
            foreach ($arch in @('x64','x86')) {
                $required = @("$arch/Hanautomata.Proofreading.exe")
                foreach ($name in @('hanautomata-proofread.exe','MANIFEST.json','LICENSE','THIRD-PARTY-NOTICES.md',
                        'data/ko.aff','data/ko.dic','data/dictionary.lock.json','data/dictionary-source.zip',
                        'data/LICENSE.md','data/LICENSE.GPL-3')) { $required += "$arch/proofreading/$name" }
                foreach ($name in $required) {
                    if ($extra -cnotcontains $name) { throw "Incomplete proofreading payload: $name" }
                }
            }
        }
    }
    $base + $extra
}
function Assert-HanautomataRegularPath([string]$Source, [string]$Relative) {
    $path = $Source
    foreach ($part in @('') + $Relative.Split('/')) {
        if ($part) { $path = Join-Path $path $part }
        if (((Get-Item -LiteralPath $path -Force -ErrorAction Stop).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Reparse payload refused: $Relative"
        }
    }
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
    Assert-HanautomataRegularPath $Source 'sha256.json'
    $manifest = Get-Content -LiteralPath (Join-Path $Source 'sha256.json') -Raw | ConvertFrom-Json
    $files = @(Get-HanautomataPayloadFiles $manifest)
    foreach ($arch in @('x64','x86')) {
        if (((Test-Path -LiteralPath (Join-Path $Source "$arch/Hanautomata.Proofreading.exe")) -or
             (Test-Path -LiteralPath (Join-Path $Source "$arch/proofreading"))) -and
            $files -cnotcontains "$arch/Hanautomata.Proofreading.exe") { throw "Unlisted proofreading payload: $arch" }
    }
    foreach ($relative in $files) {
        $expected = [string]$manifest.$relative
        $path = Join-Path $Source $relative
        if ($expected -notmatch '^[A-Fa-f0-9]{64}$' -or -not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing or invalid payload: $relative" }
        Assert-HanautomataRegularPath $Source $relative
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $expected) { throw "Payload hash mismatch: $relative" }
    }
    return $manifest
}
function Copy-HanautomataPayload([string]$Source, [string]$Destination) {
    $manifest = Assert-HanautomataPayload $Source
    $files = @(Get-HanautomataPayloadFiles $manifest)
    # Only a new destination is allowed: loaded executables and DLLs are never overwritten.
    if (Test-Path -LiteralPath $Destination) { throw "Payload destination already exists: $Destination" }
    $null = New-Item -ItemType Directory -Path $Destination
    foreach ($relative in $files) {
        $targetFile = Join-Path $Destination $relative
        $null = New-Item -ItemType Directory -Force -Path (Split-Path $targetFile)
        Copy-Item -LiteralPath (Join-Path $Source $relative) -Destination $targetFile
        if ((Get-FileHash -LiteralPath $targetFile -Algorithm SHA256).Hash -ne [string]$manifest.$relative) { throw "Staged hash mismatch: $relative" }
    }
    $verified = [ordered]@{}
    foreach ($relative in $files) { $verified[$relative] = [string]$manifest.$relative }
    $verified | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Destination 'sha256.json') -Encoding ascii
}
