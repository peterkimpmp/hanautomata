# S0 runtime update: stage in a new protected directory; keep loaded DLLs and old registration paths for rollback.
[CmdletBinding()]
param(
    [string]$Source = (Join-Path (Split-Path $PSScriptRoot) 'build/tsf'),
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')][string]$Revision,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
if (-not [Environment]::Is64BitProcess) { throw 'Use 64-bit PowerShell.' }
if (-not $CheckOnly) {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run from a visible administrator PowerShell. No changes made.'
    }
}
$installation = [IO.Path]::GetFullPath((Join-Path $env:ProgramFiles 'Hanautomata-TSF-S0'))
$destination = Join-Path $installation "revisions/$Revision"
if (Test-Path -LiteralPath $destination) { throw "Revision already exists; inspect its receipt before retrying: $destination" }
$guid = '{AC481E74-0295-436A-B0B8-AFA81FE9CBF1}'
$previous = [ordered]@{}
foreach ($arch in @('x64','x86')) {
    $view = if ($arch -eq 'x64') { [Microsoft.Win32.RegistryView]::Registry64 } else { [Microsoft.Win32.RegistryView]::Registry32 }
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $view)
    $key = $null
    try {
        $key = $base.OpenSubKey("Software\Classes\CLSID\$guid\InprocServer32")
        if ($null -eq $key) { throw "Missing $arch machine COM registration; use install-machine.ps1 for first installation." }
        $dll = [string]$key.GetValue('')
    } finally {
        if ($null -ne $key) { $key.Dispose() }
        $base.Dispose()
    }
    if (-not $dll -or -not (Test-Path -LiteralPath $dll -PathType Leaf)) { throw "Missing previous $arch DLL." }
    $dll = (Resolve-Path -LiteralPath $dll).Path
    if (-not $dll.StartsWith($installation + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Previous $arch DLL is outside the S0 installation; refusing to replace its registration."
    }
    $previous[$arch] = $dll
}
$manifestPath = Join-Path $Source 'sha256.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
foreach ($arch in @('x64','x86')) {
    foreach ($name in @('hanautomata-ime-setup.exe','hanautomata_tsf.dll')) {
        $relative = "$arch/$name"
        $expected = [string]$manifest.$relative
        if ($expected -notmatch '^[A-Fa-f0-9]{64}$' -or (Get-FileHash -LiteralPath (Join-Path $Source $relative) -Algorithm SHA256).Hash -ne $expected) {
            throw "Payload hash mismatch: $relative"
        }
    }
}
if ($CheckOnly) {
    [ordered]@{ outcome='preflight-passed'; destination=$destination; previous=$previous; changes='none' } | ConvertTo-Json -Depth 4
    return
}
$attempted = @()
$events = [Collections.Generic.List[string]]::new()
$receiptPath = Join-Path $destination 'update-result.json'
function Invoke-StagedSetup([string]$arch, [string]$operation, [string]$dll) {
    $exe = Join-Path $destination "$arch/hanautomata-ime-setup.exe"
    $output = & $exe $operation --dll $dll 2>&1 | Out-String
    $code = $LASTEXITCODE
    $events.Add("$arch $operation exit=$code`n$output")
    Write-Output $output
    if ($code -ne 0) { throw "$arch $operation failed: exit=$code" }
}
function Write-Receipt([string]$outcome, [string]$detail) {
    [ordered]@{
        revision=$Revision; outcome=$outcome; utc=[DateTime]::UtcNow.ToString('o')
        detail=$detail; previous=$previous; destination=$destination; events=@($events.ToArray())
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $receiptPath -Encoding UTF8
}
New-Item -ItemType Directory -Path $destination | Out-Null
try {
    foreach ($arch in @('x64','x86')) {
        New-Item -ItemType Directory -Path (Join-Path $destination $arch) | Out-Null
        foreach ($name in @('hanautomata-ime-setup.exe','hanautomata_tsf.dll')) {
            $relative = "$arch/$name"
            $targetFile = Join-Path $destination $relative
            Copy-Item -LiteralPath (Join-Path $Source $relative) -Destination $targetFile
            if ((Get-FileHash -LiteralPath $targetFile -Algorithm SHA256).Hash -ne $manifest.$relative) { throw "Staged hash mismatch: $relative" }
        }
        Invoke-StagedSetup $arch 'selftest' (Join-Path $destination "$arch/hanautomata_tsf.dll")
    }
    Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $destination 'sha256.json')
    foreach ($arch in @('x64','x86')) {
        $attempted += $arch
        Invoke-StagedSetup $arch 'register-machine' (Join-Path $destination "$arch/hanautomata_tsf.dll")
    }
    Write-Receipt 'updated-restart-apps-required' 'Old DLL files retained. Save and reopen test applications before validating. No user input method selection, logoff, reboot, or process termination performed.'
} catch {
    $reason = $_.Exception.Message
    $rollbackFailed = $false
    foreach ($arch in $attempted) {
        try { Invoke-StagedSetup $arch 'register-machine' $previous[$arch] }
        catch { $rollbackFailed = $true; $events.Add($_.Exception.Message) }
    }
    Write-Receipt $(if ($rollbackFailed) { 'rollback-incomplete' } else { 'failed-previous-paths-restored' }) $reason
    # Keep all files, including potentially loaded DLLs and diagnostic evidence.
    throw $reason
}
Write-Output "Updated machine registration. Receipt: $receiptPath"
