param([string]$SetupTool = (Join-Path (Split-Path $PSScriptRoot) 'build/tsf/x64/hanautomata-ime-setup.exe'))
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if ((New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'This negative regression must run without elevation.'
}
$guid = '{AC481E74-0295-436A-B0B8-AFA81FE9CBF1}'
$paths = @("HKCU:/Software/Classes/CLSID/$guid", "HKLM:/Software/Classes/CLSID/$guid", "HKCU:/Software/Microsoft/CTF/TIP/$guid", "HKLM:/Software/Microsoft/CTF/TIP/$guid")
$before = @($paths | ForEach-Object { Test-Path -LiteralPath $_ })
# The cmdlet returns a non-enumerated List on this host; Select-Object would silently produce null fields.
$languages = Get-WinUserLanguageList | ConvertTo-Json -Depth 5 -Compress
foreach ($operation in @('register-machine', 'unregister-machine')) {
    $output = & $SetupTool $operation 2>&1 | Out-String
    $code = $LASTEXITCODE
    if ($code -ne 1 -or $output -notmatch '0x80070005') { throw "Regression: $operation must refuse before writes with E_ACCESSDENIED; exit=$code $output" }
}
$after = @($paths | ForEach-Object { Test-Path -LiteralPath $_ })
if (($before -join ',') -ne ($after -join ',')) { throw 'Registration state changed in the denied path.' }
if ($languages -ne (Get-WinUserLanguageList | ConvertTo-Json -Depth 5 -Compress)) { throw 'Language list changed in the denied path.' }
'PASS: machine registration/unregistration denied before writes; current-user state unchanged.'
