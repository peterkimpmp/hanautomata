param([Parameter(Mandatory=$true)][string]$OptionsExecutable)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
. (Join-Path $root 'tsf/payload.ps1')
$buildRoot = [IO.Path]::GetFullPath((Join-Path $root 'build/windows-quality'))
$null = New-Item -ItemType Directory -Force -Path $buildRoot
$testRoot = Join-Path $buildRoot ('payload-test-' + [Guid]::NewGuid().ToString('N'))
$source = Join-Path $testRoot 'source'
$count = 0
function Assert-Payload([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "Payload regression: $Message" }
    $script:count++
}
function Assert-Rejected([scriptblock]$Action, [string]$Message) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    Assert-Payload $rejected $Message
}
try {
    foreach ($folder in @('x64','x86','ui')) { $null = New-Item -ItemType Directory -Force -Path (Join-Path $source $folder) }
    $hashes = [ordered]@{}
    foreach ($relative in Get-HanautomataPayloadFiles) {
        $file = Join-Path $source $relative
        Set-Content -LiteralPath $file -Value ('synthetic non-executable payload: ' + $relative) -Encoding utf8
        $hashes[$relative] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
    }
    $hashes | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $source 'sha256.json') -Encoding ascii
    $destination = Join-Path $testRoot 'staged'
    Copy-HanautomataPayload $source $destination
    $null = Assert-HanautomataPayload $destination
    Assert-Payload ((Get-ChildItem -LiteralPath $destination -File -Recurse).Count -eq 7) 'copy must contain only six payload files and the verified manifest'
    $previousDll = Join-Path $destination 'x64/hanautomata_tsf.dll'
    $previousTool = Join-Path $destination 'x64/hanautomata-ime-setup.exe'
    Assert-Payload ((Get-HanautomataRollbackTool $previousDll $destination) -eq $previousTool) 'rollback did not choose the previous revision tool'
    Assert-Rejected { Get-HanautomataRollbackTool $previousDll $source } 'rollback must refuse a previous DLL outside the installation'
    $oldToolBytes = [IO.File]::ReadAllBytes($previousTool)
    Remove-Item -LiteralPath $previousTool
    Assert-Rejected { Get-HanautomataRollbackTool $previousDll $destination } 'missing rollback tool must be caught before update'
    [IO.File]::WriteAllBytes($previousTool, $oldToolBytes)
    Assert-Rejected { Copy-HanautomataPayload $source $destination } 'existing revision must never be replaced'
    $null = Assert-HanautomataPayload $destination
    $editor = Join-Path $source 'ui/Hanautomata.ImeOptions.exe'
    $original = [IO.File]::ReadAllBytes($editor)
    Set-Content -LiteralPath $editor -Value 'tampered'
    $badDestination = Join-Path $testRoot 'bad-stage'
    Assert-Rejected { Copy-HanautomataPayload $source $badDestination } 'editor mismatch must fail before staging'
    Assert-Payload (-not (Test-Path -LiteralPath $badDestination)) 'bad payload created a destination'
    [IO.File]::WriteAllBytes($editor, $original)
    $config = Join-Path $source 'ui/Hanautomata.ImeOptions.exe.config'
    $configBytes = [IO.File]::ReadAllBytes($config)
    Remove-Item -LiteralPath $config
    Assert-Rejected { Copy-HanautomataPayload $source $badDestination } 'missing runtime config must be rejected'
    Assert-Payload (-not (Test-Path -LiteralPath $badDestination)) 'incomplete payload created a destination'
    [IO.File]::WriteAllBytes($config, $configBytes)
    Set-Content -LiteralPath (Join-Path $source 'private-canary.txt') -Value 'not for packaging'
    $zip = & (Join-Path $root 'tsf/package.ps1') -Source $source -OutputDirectory (Join-Path $testRoot 'packages')
    $unpacked = Join-Path $testRoot 'unpacked'
    Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
    $null = Assert-HanautomataPayload $unpacked
    Assert-Payload ((Get-ChildItem -LiteralPath $unpacked -File -Recurse).Count -eq 11) 'package has missing or unexpected files'
    foreach ($name in @('install-machine.ps1','update-machine.ps1','payload.ps1','README.md')) {
        Assert-Payload (Test-Path -LiteralPath (Join-Path $unpacked $name)) ('portable dependency missing: ' + $name)
    }
    Assert-Payload (-not (Test-Path -LiteralPath (Join-Path $unpacked 'private-canary.txt'))) 'unlisted source leaked into package'
    $sum = (Get-Content -LiteralPath ($zip + '.sha256')) -split '  ',2
    Assert-Payload ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -eq $sum[0]) 'archive digest mismatch'
    Assert-Payload ((Get-ChildItem -LiteralPath (Split-Path $zip) -Directory -Force).Count -eq 0) 'owned staging directory was not cleaned'

    # A real, headless editor invocation against this run's fixture; never the user's live settings.
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $elevated = ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $hangul = [string][char]0xd55c + [char]0xae00
    $settings = Join-Path $testRoot ("current user $hangul settings.json")
    [IO.File]::WriteAllText($settings, '{ "InputMode":"English", "ExcludedApps":["private.exe"] }')
    $before = (Get-FileHash -LiteralPath $settings -Algorithm SHA256).Hash
    $beforeAcl = (Get-Acl -LiteralPath $settings).Sddl
    $process = [Diagnostics.Process]::new()
    try {
        $process.StartInfo.FileName = (Resolve-Path -LiteralPath $OptionsExecutable).Path
        $process.StartInfo.Arguments = '--prepare-user --settings-file "' + $settings + '"'
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.CreateNoWindow = $true
        if (-not $process.Start()) { throw 'Preparation process did not start.' }
        if (-not $process.WaitForExit(15000)) { $process.Kill(); $process.WaitForExit(); throw 'Owned preparation child timed out.' }
        $expectedExit = if ($elevated) { 3 } else { 0 }
        Assert-Payload ($process.ExitCode -eq $expectedExit) 'headless preparation did not honor the caller privilege boundary'
    } finally { $process.Dispose() }
    Assert-Payload ((Get-FileHash -LiteralPath $settings -Algorithm SHA256).Hash -eq $before) 'preparation changed existing options'
    Assert-Payload (-not (Test-Path -LiteralPath ($settings + '.previous'))) 'preparation rotated history'
    if ($elevated) {
        Assert-Payload ((Get-Acl -LiteralPath $settings).Sddl -eq $beforeAcl) 'elevated preparation changed access'
    } else {
        $rights = [Security.AccessControl.FileSystemRights]0
        foreach ($rule in (Get-Acl -LiteralPath $settings).GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
            if ($rule.IdentityReference.Value -eq 'S-1-15-2-1' -and $rule.AccessControlType -eq 'Allow') { $rights = $rights -bor $rule.FileSystemRights }
        }
        $read = [Security.AccessControl.FileSystemRights]::Read -bor [Security.AccessControl.FileSystemRights]::Synchronize
        $write = [Security.AccessControl.FileSystemRights]::Write -bor [Security.AccessControl.FileSystemRights]::Delete -bor [Security.AccessControl.FileSystemRights]::ChangePermissions
        Assert-Payload (($rights -band $read) -eq $read -and ($rights -band $write) -eq 0) 'legacy settings were not migrated to package read-only access'
    }
    Write-Output "PASS: $count TSF payload and user-preparation assertions; 0 failures; elevated=$elevated (elevated checks refusal only)"
} finally {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $buildRoot -or [IO.Path]::GetFileName($resolved) -notmatch '^payload-test-[a-f0-9]{32}$') { throw 'Unsafe payload test cleanup path.' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
