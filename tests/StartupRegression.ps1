param([string]$SourceDirectory)
$ErrorActionPreference = 'Stop'
if (-not $SourceDirectory) { $SourceDirectory = Join-Path (Split-Path -Parent $PSScriptRoot) 'build' }
# Run in a dedicated Windows PowerShell process: the HKCU override is process-local.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class StartupRegistrySandbox {
    [DllImport("advapi32.dll")] public static extern int RegOverridePredefKey(IntPtr key, IntPtr replacement);
}
'@
Add-Type -AssemblyName System.Windows.Forms
$assembly = [Reflection.Assembly]::LoadFrom((Join-Path $SourceDirectory 'Hanautomata.exe'))
$sandboxPath = 'Software\HanautomataStartupTests\' + [Guid]::NewGuid().ToString('N')
$sandbox = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($sandboxPath)
$overridden = $false
try {
    $result = [StartupRegistrySandbox]::RegOverridePredefKey([IntPtr](-2147483647), $sandbox.Handle.DangerousGetHandle())
    if ($result -ne 0) { throw "Registry sandbox failed: $result" }
    $overridden = $true
    $run = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Microsoft\Windows\CurrentVersion\Run')
    try { $run.SetValue('Hanautomata', ('"' + [Windows.Forms.Application]::ExecutablePath + '" --background')) } finally { $run.Dispose() }
    $approval = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run')
    try { $approval.SetValue('Hanautomata', [byte[]]@(3,0,0,0,0,0,0,0,0,0,0,0), [Microsoft.Win32.RegistryValueKind]::Binary) } finally { $approval.Dispose() }
    $legacyMethod = $assembly.GetType('Hanautomata.TrayApplication').GetMethod('StartupEnabled', [Reflection.BindingFlags]'NonPublic,Static')
    if ($legacyMethod) { $enabled = $legacyMethod.Invoke($null, @()) }
    else {
        $method = $assembly.GetType('Hanautomata.StartupRegistration').GetMethod('LegacyEnabled', [Reflection.BindingFlags]'NonPublic,Static')
        $enabled = $method.Invoke($null, @('Hanautomata'))
    }
    if ($enabled) { throw 'REGRESSION: Windows-disabled startup is incorrectly reported as enabled.' }
    'PASS: Windows-disabled legacy startup is not reported as enabled.'
} finally {
    if ($overridden) { $null = [StartupRegistrySandbox]::RegOverridePredefKey([IntPtr](-2147483647), [IntPtr]::Zero) }
    $sandbox.Dispose()
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($sandboxPath, $false)
}
