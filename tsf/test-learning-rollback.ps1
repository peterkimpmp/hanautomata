# Actual old/new core executables; dedicated synthetic files only, no installation or user settings.
param([string]$LegacyCommit = '6519f59d6cace880e73a9ba4d61c4dc4dea33e1d')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$run = Join-Path $root ('build/windows-quality/rollback-compat-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run | Out-Null
$archive = Join-Path $run 'legacy-source.zip'
& git -C $root archive --format=zip "--output=$archive" $LegacyCommit core data
if ($LASTEXITCODE -ne 0) { throw 'Could not archive the specified legacy commit.' }
Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $run 'legacy-source')
$source = Join-Path $PSScriptRoot 'tests/fixtures/learning_compat_probe.rs'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$oldTarget = $env:CARGO_TARGET_DIR
$targetBase = if ($oldTarget) { $oldTarget } else { Join-Path $run 'target' }
$executables = @{}
try {
    foreach ($kind in @('legacy', 'current')) {
        $project = Join-Path $run $kind
        New-Item -ItemType Directory -Path $project | Out-Null
        $corePath = if ($kind -eq 'legacy') { Join-Path $run 'legacy-source/core' } else { Join-Path $root 'core' }
        $coreLiteral = ConvertTo-Json ($corePath.Replace('\', '/')) -Compress
        $sourceLiteral = ConvertTo-Json ($source.Replace('\', '/')) -Compress
        $manifest = @"
[package]
name = "learning-compat-probe"
version = "0.0.0"
edition = "2021"
[features]
managed = []
[[bin]]
name = "learning-compat-probe"
path = $sourceLiteral
[dependencies]
hanautomata-core = { path = $coreLiteral }
serde_json = "1"
sha2 = "0.10"
"@
        $manifestPath = Join-Path $project 'Cargo.toml'
        [IO.File]::WriteAllText($manifestPath, $manifest, $utf8)
        $env:CARGO_TARGET_DIR = Join-Path $targetBase ('rollback-compat-' + $kind)
        $arguments = @('build', '--release', '--manifest-path', $manifestPath)
        if ($kind -eq 'current') { $arguments += @('--features', 'managed') }
        # PowerShell 5.1 represents native stderr (including normal cargo progress) as ErrorRecord.
        # Judge the native exit code; do not stop on a compiler progress line.
        $previousErrorAction = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            & cargo @arguments *> (Join-Path $run ($kind + '-build.log'))
            $buildExit = $LASTEXITCODE
        } finally { $ErrorActionPreference = $previousErrorAction }
        if ($buildExit -ne 0) { throw "Compatibility probe build failed: $kind; see $run" }
        $executables[$kind] = Join-Path $env:CARGO_TARGET_DIR 'release/learning-compat-probe.exe'
    }
} finally { $env:CARGO_TARGET_DIR = $oldTarget }
$store = Join-Path $run 'learning.db'
function Invoke-Probe([string]$kind, [string]$action) {
    $output = & $executables[$kind] $action $store
    if ($LASTEXITCODE -ne 0) { throw "$kind $action failed" }
    return ($output | ConvertFrom-Json)
}
$before = Invoke-Probe legacy before
$managed = Invoke-Probe current managed
$edges = Invoke-Probe current edges
$boundary = Invoke-Probe current boundary
$hashBeforeRefusal = (Get-FileHash -LiteralPath $store -Algorithm SHA256).Hash
# cmd redirects expected stderr without PowerShell 5.1 treating it as a terminating error.
$refusalLog = Join-Path $run 'legacy-refusal.log'
$legacy = $executables['legacy']
& cmd /d /c "`"$legacy`" read `"$store`" > `"$refusalLog`" 2>&1"
$refusal = $LASTEXITCODE
if ($refusal -ne 3 -or (Get-Content -LiteralPath $refusalLog -Raw) -notmatch 'InvalidState' -or
    (Get-FileHash -LiteralPath $store -Algorithm SHA256).Hash -ne $hashBeforeRefusal) {
    throw 'An old reader must refuse managed metadata without changing the store.'
}
$drained = Invoke-Probe current drain
$restored = Invoke-Probe legacy read
$after = Invoke-Probe legacy after
$final = Invoke-Probe current read
if ((($restored.entries.raw | Sort-Object) -join ',') -ne 'before,dkssud,dkssud,retained') { throw 'Rollback lost learned entries.' }
if ((($final.entries.raw | Sort-Object) -join ',') -ne 'after,before,dkssud,dkssud,retained') { throw 'Old/new writes did not preserve all entries.' }
if (($restored.entries | Where-Object { $_.raw -eq 'dkssud' -and $_.context -ne '' }).context.Length -ne 64 -or $restored.version.generation -le 1 -or
    ($restored.entries | Where-Object raw -EQ 'before').korean_weight -ne 16 -or
    ($restored.entries | Where-Object raw -EQ 'retained').english_weight -ne 16) { throw 'Context, fence or weight boundary was lost.' }
if ($before.version.store_id -ne $final.version.store_id -or $final.version.revision -le $managed.version.revision) {
    throw 'Rollback changed identity or regressed revision.'
}
$reset = Invoke-Probe current reset
$resetOld = Invoke-Probe legacy read
$afterReset = Invoke-Probe legacy after
$finalReset = Invoke-Probe current read
if ($resetOld.entries.Count -ne 0 -or $resetOld.version.generation -le $final.version.generation -or
    $finalReset.entries.Count -ne 1 -or $finalReset.entries[0].raw -ne 'after') { throw 'Reset fence did not survive old/new readers.' }
$receipt = [ordered]@{ legacy_commit=$LegacyCommit; legacy_archive_sha256=(Get-FileHash $archive -Algorithm SHA256).Hash;
    legacy_refusal_exit=$refusal; legacy_refusal_preserved_bytes=$true; restored_entries=$restored.entries.Count;
    final_entries=$final.entries.Count; preserved_store_id=$true; final_revision=$final.version.revision;
    korean_context=$true; forget_fence=$true; weight_boundaries_0_16=$true; reset_fence=$true;
    post_reset_entries=$finalReset.entries.Count; post_reset_revision=$finalReset.version.revision;
    probe_sha256=(Get-FileHash $source -Algorithm SHA256).Hash; result='passed' }
$receiptPath = Join-Path $run 'result.json'
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 4), $utf8)
Write-Output "PASS: old -> managed -> old -> new, 5 entries; Korean context, forget/reset fences and 0/16 weights. Receipt: $receiptPath"
