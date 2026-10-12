# S0 only. Run once as administrator; enable-user runs separately as the intended normal user.
param(
    [ValidateSet('Install', 'Unregister')][string]$Action = 'Install',
    [string]$Source = (Join-Path (Split-Path $PSScriptRoot) 'build/tsf')
)
$ErrorActionPreference = 'Stop'
# Rust writes UTF-8. Windows PowerShell 5.1 otherwise decodes native output with the OEM code page.
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an elevated PowerShell. No changes made.'
}
if (-not [Environment]::Is64BitProcess) { throw 'Use 64-bit PowerShell for installation.' }
$destination = Join-Path $env:ProgramFiles 'Hanautomata-TSF-S0'
$receiptPath = Join-Path $destination 'install-result.json'
$guid = '{AC481E74-0295-436A-B0B8-AFA81FE9CBF1}'
$attempted = @()
$events = [Collections.Generic.List[string]]::new()
function Invoke-Setup([string]$arch, [string]$operation) {
    $exe = Join-Path $destination "$arch/hanautomata-ime-setup.exe"
    $output = & $exe $operation 2>&1 | Out-String
    $code = $LASTEXITCODE
    $events.Add("$arch $operation exit=$code`n$output")
    Write-Output $output
    if ($code -ne 0) { throw "$arch $operation failed: exit=$code" }
}
function Write-Receipt([string]$outcome, [string]$detail) {
    [ordered]@{ action=$Action; outcome=$outcome; utc=[DateTime]::UtcNow.ToString('o'); detail=$detail; events=@($events.ToArray()) } |
        ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $receiptPath -Encoding UTF8
}
if ($Action -eq 'Install') {
    # Refuse an upgrade/replacement: S0 rollback is only safe against an absent registration.
    foreach ($key in @("HKLM:/Software/Classes/CLSID/$guid", "HKLM:/Software/Classes/WOW6432Node/CLSID/$guid", "HKLM:/Software/Microsoft/CTF/TIP/$guid", "HKLM:/Software/WOW6432Node/Microsoft/CTF/TIP/$guid")) {
        if (Test-Path -LiteralPath $key) { throw "Existing registration; refusing to replace $key" }
    }
    if (Test-Path -LiteralPath $destination) { throw "Existing staging folder; inspect before replacing: $destination" }
    $manifest = Get-Content -LiteralPath (Join-Path $Source 'sha256.json') -Raw | ConvertFrom-Json
    foreach ($arch in @('x64','x86')) {
        foreach ($name in @('hanautomata-ime-setup.exe','hanautomata_tsf.dll')) {
            $relative = "$arch/$name"
            $expected = $manifest.$relative
            if (-not $expected -or (Get-FileHash -LiteralPath (Join-Path $Source $relative) -Algorithm SHA256).Hash -ne $expected) {
                throw "Payload hash mismatch: $relative"
            }
        }
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
            Invoke-Setup $arch 'selftest'
        }
        Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $destination 'install-machine.ps1')
        Copy-Item -LiteralPath (Join-Path $Source 'sha256.json') -Destination (Join-Path $destination 'sha256.json')
        foreach ($arch in @('x64','x86')) {
            $attempted += $arch
            Invoke-Setup $arch 'register-machine'
        }
        Write-Receipt 'installed' 'Machine registration only. Run enable-user in the intended normal account.'
    } catch {
        $reason = $_.Exception.Message
        $rollbackFailed = $false
        foreach ($arch in $attempted) {
            try { Invoke-Setup $arch 'unregister-machine' } catch { $rollbackFailed = $true; $events.Add($_.Exception.Message) }
        }
        Write-Receipt $(if ($rollbackFailed) { 'rollback-incomplete' } else { 'failed-registration-rolled-back' }) $reason
        # Keep staged files and receipt for diagnosis; never delete a potentially loaded DLL.
        throw $reason
    }
} else {
    if (-not (Test-Path -LiteralPath (Join-Path $destination 'sha256.json'))) { throw 'No S0 installation manifest found.' }
    $failed = $false
    foreach ($arch in @('x64','x86')) {
        try { Invoke-Setup $arch 'unregister-machine' } catch { $failed = $true; $events.Add($_.Exception.Message) }
    }
    Write-Receipt $(if ($failed) { 'unregister-incomplete' } else { 'unregistered' }) 'Staged files retained; user must disable-user before machine removal.'
    if ($failed) { throw 'Machine unregistration incomplete; read install-result.json.' }
}
