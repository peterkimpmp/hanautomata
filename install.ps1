param([switch]$Startup, [switch]$NoLaunch, [switch]$ValidateOnly, [string]$SourceDirectory)
$ErrorActionPreference = 'Stop'
if (-not $SourceDirectory) {
    $SourceDirectory = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'Hanautomata.exe')) { $PSScriptRoot } else { Join-Path $PSScriptRoot 'build' }
}
$sourceExe = Join-Path $SourceDirectory 'Hanautomata.exe'
if (-not (Test-Path -LiteralPath $sourceExe)) { throw 'Build Hanautomata first, or specify -SourceDirectory with the portable package.' }
$config = Join-Path $SourceDirectory 'Hanautomata.exe.config'
if (-not (Test-Path -LiteralPath $config)) { throw 'Hanautomata.exe.config is required beside the executable.' }
if ($ValidateOnly) { Write-Output 'Install source validated; no files, shortcuts or startup entries changed.'; return }
$destination = [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs/Hanautomata'))
$null = New-Item -ItemType Directory -Force -Path $destination
$installedExe = Join-Path $destination 'Hanautomata.exe'
if (Test-Path -LiteralPath $installedExe) {
    $exitProcess = Start-Process -FilePath $installedExe -ArgumentList '--exit' -WindowStyle Hidden -PassThru
    $null = $exitProcess.WaitForExit(5000)
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $running = @(Get-Process -Name Hanautomata -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $installedExe })
        if ($running.Count -eq 0) { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($running.Count -gt 0) { throw 'Close Hanautomata from the tray and retry.' }
}
$legacyRunning = @(Get-Process -Name HanFlow -ErrorAction SilentlyContinue)
if ($legacyRunning.Count -gt 0) { throw 'Close HanFlow (the previous name of this app) from the tray first; two instances would both hook the keyboard.' }
$legacyInstall = Join-Path $env:LOCALAPPDATA 'Programs/HanFlow'
if (Test-Path -LiteralPath $legacyInstall) { Write-Warning "Previous HanFlow install left in place: $legacyInstall (and HanFlow.lnk shortcuts). Remove them after confirming Hanautomata works. Settings and learned words are copied to %LOCALAPPDATA%\Hanautomata on first start." }
Copy-Item -LiteralPath $sourceExe -Destination $installedExe -Force
Copy-Item -LiteralPath $config -Destination (Join-Path $destination 'Hanautomata.exe.config') -Force
if (-not ('HanautomataInstall.Shortcut' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
namespace HanautomataInstall {
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")] class ShellLink {}
    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellLinkW {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size, IntPtr data, uint flags);
        void GetIDList(out IntPtr list);
        void SetIDList(IntPtr list);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder value, int size);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string value);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string path);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder value, int size);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string value);
        void GetHotkey(out short key);
        void SetHotkey(short key);
        void GetShowCmd(out int command);
        void SetShowCmd(int command);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string path, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved);
        void Resolve(IntPtr window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string path);
    }
    public static class Shortcut {
        public static void Create(string path, string target, string directory) {
            var link = (IShellLinkW)new ShellLink();
            try {
                link.SetPath(target); link.SetWorkingDirectory(directory);
                link.SetDescription("Hanautomata - local Korean and English automatic input");
                link.SetIconLocation(target, 0); link.SetShowCmd(1);
                ((IPersistFile)link).Save(path, true);
            } finally { Marshal.FinalReleaseComObject(link); }
        }
        public static string ReadTarget(string path) {
            var link = (IShellLinkW)new ShellLink();
            try { ((IPersistFile)link).Load(path, 0); var target = new StringBuilder(32768); link.GetPath(target, target.Capacity, IntPtr.Zero, 0); return target.ToString(); }
            finally { Marshal.FinalReleaseComObject(link); }
        }
    }
}
'@
}
foreach ($shortcutDirectory in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) {
    $shortcutPath = [System.IO.Path]::GetFullPath((Join-Path $shortcutDirectory 'Hanautomata.lnk'))
    [HanautomataInstall.Shortcut]::Create($shortcutPath, $installedExe, $destination)
    if ([HanautomataInstall.Shortcut]::ReadTarget($shortcutPath) -ne $installedExe) { throw 'Shortcut target verification failed.' }
}
if ($Startup) {
    $runKey = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
    $null = New-Item -Path $runKey -Force
    Set-ItemProperty -Path $runKey -Name 'Hanautomata' -Value ('"' + $installedExe + '" --background')
    Remove-ItemProperty -Path $runKey -Name 'HanFlow' -ErrorAction SilentlyContinue
}
if (-not $NoLaunch) { Start-Process -FilePath $installedExe -WindowStyle Hidden }
Write-Output "Installed for current user: $installedExe"
