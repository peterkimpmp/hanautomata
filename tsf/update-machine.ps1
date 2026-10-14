# S0 runtime update: stage in a new protected directory; keep loaded DLLs and old registration paths for rollback.
[CmdletBinding()]
param(
    [string]$Source,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')][string]$Revision,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'payload.ps1')
if (-not $Source) { $Source = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'sha256.json')) { $PSScriptRoot } else { Join-Path (Split-Path $PSScriptRoot) 'build/tsf' } }
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
$previousTools = [ordered]@{}
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
    $previousTools[$arch] = Get-HanautomataRollbackTool $dll $installation
}
$null = Assert-HanautomataPayload $Source
if ($CheckOnly) {
    [ordered]@{ outcome='preflight-passed'; destination=$destination; previous=$previous; rollback_tools=$previousTools; changes='none' } | ConvertTo-Json -Depth 4
    return
}
$attempted = @()
$events = [Collections.Generic.List[string]]::new()
$receiptPath = Join-Path $destination 'update-result.json'
function Invoke-StagedSetup([string]$arch, [string]$operation, [string]$dll, [string]$setupPath) {
    $exe = if ($setupPath) { $setupPath } else { Join-Path $destination "$arch/hanautomata-ime-setup.exe" }
    $output = & $exe $operation --dll $dll 2>&1 | Out-String
    $code = $LASTEXITCODE
    $events.Add("$arch $operation tool=$exe exit=$code`n$output")
    Write-Output $output
    if ($code -ne 0) { throw "$arch $operation failed: exit=$code" }
}
function Write-Receipt([string]$outcome, [string]$detail) {
    [ordered]@{
        revision=$Revision; outcome=$outcome; utc=[DateTime]::UtcNow.ToString('o')
        detail=$detail; previous=$previous; rollback_tools=$previousTools; destination=$destination; events=@($events.ToArray())
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $receiptPath -Encoding UTF8
}
try {
    Copy-HanautomataPayload $Source $destination
    foreach ($arch in @('x64','x86')) {
        Invoke-StagedSetup $arch 'selftest' (Join-Path $destination "$arch/hanautomata_tsf.dll")
    }
    foreach ($arch in @('x64','x86')) {
        $attempted += $arch
        Invoke-StagedSetup $arch 'register-machine' (Join-Path $destination "$arch/hanautomata_tsf.dll")
    }
    Write-Receipt 'updated-user-preparation-required' 'Old files retained. Each intended user must run this revision x64/hanautomata-ime-setup.exe enable-user in a normal session, then save and reopen test applications. No user selection, logoff, reboot, or process termination performed.'
} catch {
    $reason = $_.Exception.Message
    $rollbackFailed = $false
    foreach ($arch in $attempted) {
        try { Invoke-StagedSetup $arch 'register-machine' $previous[$arch] $previousTools[$arch] }
        catch { $rollbackFailed = $true; $events.Add($_.Exception.Message) }
    }
    if (Test-Path -LiteralPath $destination) { Write-Receipt $(if ($rollbackFailed) { 'rollback-incomplete' } elseif ($attempted.Count -eq 0) { 'failed-before-registration' } else { 'failed-previous-paths-restored' }) $reason }
    # Keep all files, including potentially loaded DLLs and diagnostic evidence.
    throw $reason
}
Write-Output "Updated machine registration. Receipt: $receiptPath"
