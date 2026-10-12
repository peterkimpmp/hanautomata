# S0-4 without a person at the screen: registers the text service for this user (--ctf-hkcu), switches this session to
# it, types into a test window with synthetic keys (SendKeys), reads the text back, then switches back and unregisters.
# Synthetic keys are not a physical-keyboard result (C-CTX-08): this answers whether Windows loads and runs the text
# service at all. Keys are sent only while the test window is the active window.
#   .\tsf\s0-live.ps1 [-Out C:\dev\s0-live] [-KeepRegistered]
param(
    [string]$Out = 'C:\dev\s0-live',
    [switch]$KeepRegistered
)

$root = Split-Path $PSScriptRoot
$tool = Join-Path $root 'build\tsf\x64\hanautomata-ime-setup.exe'
$log = Join-Path $env:LOCALAPPDATA 'Hanautomata\ime-s0.log'
New-Item -ItemType Directory -Force $Out | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

function Step($name) { "`n== $name" }
function LogLines { if (Test-Path $log) { @(Get-Content $log -Encoding UTF8) } else { @() } }
function Pump($ms) { $end = (Get-Date).AddMilliseconds($ms); while ((Get-Date) -lt $end) { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 } }
function Keys { Get-WinUserLanguageList | ForEach-Object { "$($_.LanguageTag): $($_.InputMethodTips -join ', ')" } }
function Export($tag) {
    reg export "HKCU\Control Panel\International\User Profile" "$Out\up-$tag.reg" /y | Out-Null
    reg export "HKCU\Software\Microsoft\CTF" "$Out\ctf-$tag.reg" /y | Out-Null
    reg export "HKCU\Keyboard Layout" "$Out\kbl-$tag.reg" /y | Out-Null
}

Step 'session'
[Environment]::OSVersion.VersionString
try { qwinsta 2>&1 } catch { "qwinsta: $_" }
Get-Process | Where-Object { $_.Name -like 'Hanautomata*' } | Format-Table Id, Name, Path -AutoSize | Out-String
if (-not (Test-Path $tool)) { "missing $tool - build first (tsf\build.ps1)"; exit 2 }

Step 'before'
Export 'before'
$activeBefore = & $tool active
$activeBefore
$before = ($activeBefore | Select-String 'active keyboard input method (\S+)').Matches | ForEach-Object { $_.Groups[1].Value } | Select-Object -First 1
Keys
$logStart = (LogLines).Count

$form = $null
$results = @()
try {
    Step 'register --ctf-hkcu'
    & $tool register --ctf-hkcu

    Step 'test window'
    $form = New-Object Windows.Forms.Form
    $form.Text = 'Hanautomata S0-4'
    $form.TopMost = $true
    $form.Width = 640
    $form.Height = 180
    $box = New-Object Windows.Forms.TextBox
    $box.Multiline = $true
    $box.Dock = 'Fill'
    $box.Font = New-Object Drawing.Font('Malgun Gothic', 20)
    $form.Controls.Add($box)
    $form.Show()
    $form.Activate()
    Pump 300
    if ([Windows.Forms.Form]::ActiveForm -ne $form) {
        (New-Object -ComObject WScript.Shell).AppActivate($form.Text) | Out-Null
        Pump 300
    }
    $box.Focus() | Out-Null
    "active window is the test window: $([Windows.Forms.Form]::ActiveForm -eq $form)"

    Step 'activate'
    & $tool activate
    Pump 500
    & $tool active

    $cases = @(
        @{ Keys = 'dkssud'; Expect = "$([char]0xC548)$([char]0xB155) " },
        @{ Keys = 'hello'; Expect = 'hello ' }
    )
    $n = 0
    foreach ($case in $cases) {
        $n++
        Step "type '$($case.Keys) '"
        $box.Clear()
        $box.Focus() | Out-Null
        Pump 200
        if ([Windows.Forms.Form]::ActiveForm -ne $form) { "skipped: the test window is not the active window"; $results += "case $n skipped"; continue }
        foreach ($c in $case.Keys.ToCharArray()) { [Windows.Forms.SendKeys]::SendWait([string]$c); Pump 40 }
        Pump 300
        $mid = $box.Text
        try {
            $shot = New-Object Drawing.Bitmap $form.Width, $form.Height
            [Drawing.Graphics]::FromImage($shot).CopyFromScreen($form.Location, [Drawing.Point]::Empty, $form.Size)
            $shot.Save("$Out\case$n.png")
            "screenshot $Out\case$n.png"
        } catch { "screenshot failed: $($_.Exception.Message)" }
        [Windows.Forms.SendKeys]::SendWait(' ')
        Pump 500
        $final = $box.Text
        $codes = ($final.ToCharArray() | ForEach-Object { 'U+{0:X4}' -f [int]$_ }) -join ' '
        $verdict = if ($final -eq $case.Expect) { 'PASS' } else { 'FAIL' }
        "before space: '$mid'"
        "after space:  '$final' ($codes)"
        "expected:     '$($case.Expect)' -> $verdict"
        $results += "case $n '$($case.Keys) ' -> $verdict ($codes)"
    }
}
finally {
    Step 'restore'
    if ($before) { & $tool activate --to $before } else { "no token from 'active' before the test; unregister switches away from the text service" }
    if ($form) { $form.Close(); $form.Dispose() }
    Step 'text service log (new lines)'
    LogLines | Select-Object -Skip $logStart
    if (-not $KeepRegistered) {
        Step 'unregister'
        & $tool unregister
        & $tool status
        Export 'after'
        foreach ($n in 'up', 'ctf', 'kbl') {
            $diff = Compare-Object (Get-Content "$Out\$n-before.reg") (Get-Content "$Out\$n-after.reg")
            "$n registry export: $(if ($diff) { "$(@($diff).Count) lines differ" } else { 'same as before' })"
            $diff | Select-Object -First 10 | Format-Table -AutoSize -Wrap | Out-String
        }
        Keys
    }
    & $tool active
    Step 'summary'
    $results
}
