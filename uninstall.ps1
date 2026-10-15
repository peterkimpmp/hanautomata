param([switch]$PurgeData, [switch]$KeepShortcuts)
# Removes the current user's tray-app install (%LOCALAPPDATA%\Programs\Hanautomata), its startup
# registration and shortcuts. Settings and learned words are kept unless -PurgeData is given.
# The TSF text service edition is separate: remove it with tsf\install-machine.ps1 (administrator).
$ErrorActionPreference = 'Stop'
$destination = [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs/Hanautomata'))
$installedExe = Join-Path $destination 'Hanautomata.exe'
$dataDirectory = [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Hanautomata'))

if (Test-Path -LiteralPath $installedExe) {
    # Disable startup first, while the executable that owns the scheduled task still exists.
    $startupReport = Join-Path ([IO.Path]::GetTempPath()) ('hanautomata-uninstall-' + [Guid]::NewGuid().ToString('N') + '.json')
    try {
        $configure = Start-Process -FilePath $installedExe -ArgumentList @('--startup', 'disable', ('"' + $startupReport + '"')) -PassThru -WindowStyle Hidden
        if (-not $configure.WaitForExit(15000)) { $configure.Kill(); Write-Warning 'Startup unregistration timed out; check the tray menu or Task Scheduler (\Hanautomata).' }
        elseif ($configure.ExitCode -ne 0) { Write-Warning 'Startup unregistration reported an error; check %LOCALAPPDATA%\Hanautomata\startup.log.' }
        else { Write-Output 'Startup: disabled' }
    } finally { if (Test-Path -LiteralPath $startupReport) { Remove-Item -LiteralPath $startupReport -Force } }

    $exitProcess = Start-Process -FilePath $installedExe -ArgumentList '--exit' -WindowStyle Hidden -PassThru
    $null = $exitProcess.WaitForExit(5000)
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $running = @(Get-Process -Name Hanautomata -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $installedExe })
        if ($running.Count -eq 0) { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($running.Count -gt 0) { throw 'Close Hanautomata from the tray and retry.' }
} else {
    Write-Output "No installed executable at $installedExe; removing leftovers only."
}

if (-not $KeepShortcuts) {
    foreach ($shortcutDirectory in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) {
        $shortcutPath = [System.IO.Path]::GetFullPath((Join-Path $shortcutDirectory 'Hanautomata.lnk'))
        if (Test-Path -LiteralPath $shortcutPath) {
            # Only remove shortcuts that point into this install; leave anything the user made themselves.
            $shell = New-Object -ComObject WScript.Shell
            $target = $shell.CreateShortcut($shortcutPath).TargetPath
            if ($target -and $target.StartsWith($destination, [StringComparison]::OrdinalIgnoreCase)) {
                Remove-Item -LiteralPath $shortcutPath -Force
                Write-Output "Removed shortcut: $shortcutPath"
            }
        }
    }
}

if (Test-Path -LiteralPath $destination) {
    if ([IO.Path]::GetFileName($destination) -ne 'Hanautomata') { throw 'Unsafe uninstall path.' }
    Remove-Item -LiteralPath $destination -Recurse -Force
    Write-Output "Removed install directory: $destination"
}

if ($PurgeData) {
    if (Test-Path -LiteralPath $dataDirectory) {
        if ([IO.Path]::GetFileName($dataDirectory) -ne 'Hanautomata') { throw 'Unsafe data path.' }
        Remove-Item -LiteralPath $dataDirectory -Recurse -Force
        Write-Output "Removed settings and learned words: $dataDirectory"
    }
} elseif (Test-Path -LiteralPath $dataDirectory) {
    Write-Output "Kept settings and learned words: $dataDirectory (remove with -PurgeData)"
}
Write-Output 'Uninstalled. A previous HanFlow install, if any, is untouched (see docs/MIGRATION.md).'
