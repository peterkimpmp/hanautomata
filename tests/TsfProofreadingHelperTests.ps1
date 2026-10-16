param(
    [Parameter(Mandatory=$true)][string]$HelperExecutable,
    [ValidateRange(1,60)][int]$TimeoutSeconds = 10
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $HelperExecutable).Path
$count = 0
function Assert-Helper([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "Helper pipe regression: $Message" }
    $script:count++
}
function Invoke-OwnedHelper([string]$Frame, [bool]$Valid) {
    $p = [Diagnostics.Process]::new()
    $p.StartInfo.FileName = $exe
    $p.StartInfo.WorkingDirectory = Split-Path $exe
    $p.StartInfo.UseShellExecute = $false
    $p.StartInfo.CreateNoWindow = $true
    $p.StartInfo.RedirectStandardInput = $true
    $p.StartInfo.RedirectStandardOutput = $true
    $p.StartInfo.RedirectStandardError = $true
    $p.StartInfo.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $p.StartInfo.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $ownedPid = $null
    try {
        $null = $p.Start()
        $ownedPid = $p.Id
        $outTask = $p.StandardOutput.ReadToEndAsync()
        $errTask = $p.StandardError.ReadToEndAsync()
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($Frame + [char]10)
        $p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
        $p.StandardInput.BaseStream.Flush()
        $p.StandardInput.Close()
        if ($Valid) {
            $watch = [Diagnostics.Stopwatch]::StartNew()
            do {
                $p.Refresh()
                if ($p.HasExited -or $p.MainWindowHandle -ne [IntPtr]::Zero) { break }
                Start-Sleep -Milliseconds 50
            } while ($watch.Elapsed.TotalSeconds -lt $TimeoutSeconds)
            $earlyExit = if ($p.HasExited) { $p.ExitCode } else { 'running' }
            Assert-Helper (-not $p.HasExited -and $p.MainWindowHandle -ne [IntPtr]::Zero) "valid JSON: owned window absent; PID=$ownedPid exit=$earlyExit"
            Assert-Helper ($p.CloseMainWindow()) "owned CloseMainWindow refused; PID=$ownedPid"
        }
        Assert-Helper ($p.WaitForExit($TimeoutSeconds * 1000)) "owned helper timeout; PID=$ownedPid"
        $stdout = $outTask.GetAwaiter().GetResult()
        $stderr = $errTask.GetAwaiter().GetResult()
        $expected = if ($Valid) { 0 } else { 2 }
        Assert-Helper ($p.ExitCode -eq $expected) "expected exit $expected, got $($p.ExitCode); PID=$ownedPid"
        Assert-Helper ([string]::IsNullOrEmpty($stdout)) "unexpected stdout; PID=$ownedPid"
        Write-Output "CASE valid=$Valid PID=$ownedPid exit=$($p.ExitCode) stdout-empty=True"
    } finally {
        if ($null -ne $ownedPid -and -not $p.HasExited) {
            $null = $p.CloseMainWindow()
            if (-not $p.WaitForExit(2000)) {
                # Cleanup is restricted to the exact child started by this test.
                $p.Kill()
                $null = $p.WaitForExit(2000)
                throw "Helper pipe regression: owned cleanup required termination; PID=$ownedPid"
            }
        }
        $p.Dispose()
    }
}
# Decode ASCII escapes first so Windows PowerShell also sends real UTF-8 Korean.
$valid = '{"request_id":"helper-pipe-test","document_id":"synthetic-helper","revision":"1","text":"\uc774\ub807\uac8c \ud558\uba74 \ub418\uc694.","protected_words":[]}' | ConvertFrom-Json | ConvertTo-Json -Compress
Invoke-OwnedHelper $valid $true
Invoke-OwnedHelper '{' $false
Write-Output "PASS: $count helper pipe assertions; 0 failures"
