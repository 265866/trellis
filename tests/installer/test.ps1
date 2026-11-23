# Tests the installers run.sh built: a silent per-user install, an upgrade, an upgrade while the
# helper runs with a window, with a hidden window and without one, with and without
# CloseApplications "force", an upgrade from an Inno Setup 6 build, and a silent uninstall. It
# prints each result and what an Inno Setup 6 install's uninstall entry and files show differently
# from an Inno Setup 7 one, and exits with 1 when a check fails.
param(
    [Parameter(Mandatory)] [string] $V1,
    [Parameter(Mandatory)] [string] $V1Inno6,
    [Parameter(Mandatory)] [string] $V2,
    [Parameter(Mandatory)] [string] $V2Force,
    [Parameter(Mandatory)] [string] $Helper1,
    [Parameter(Mandatory)] [string] $Helper2,
    [Parameter(Mandatory)] [string] $AppId,
    [Parameter(Mandatory)] [string] $AppName,
    [Parameter(Mandatory)] [string] $Exe,
    [Parameter(Mandatory)] [string] $Logs
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Logs | Out-Null
$Key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{$AppId}_is1"
$AppDir = Join-Path $env:LOCALAPPDATA "Programs\$AppName"
$Helper = Join-Path $AppDir 'window.exe'
$script:Failed = $false

function Check([string] $Name, [bool] $Ok, [string] $Detail = '') {
    $verdict = if ($Ok) { 'pass' } else { 'FAIL' }
    Write-Host "$verdict | $Name | $Detail"
    if (-not $Ok) { $script:Failed = $true }
}
function Hash([string] $Path) {
    if (Test-Path $Path) { (Get-FileHash $Path -Algorithm SHA256).Hash } else { 'missing' }
}
function Run([string] $File, [string[]] $Arguments, [int] $Seconds = 180) {
    $start = [Diagnostics.ProcessStartInfo]::new($File, ($Arguments -join ' '))
    $start.UseShellExecute = $false
    $process = [Diagnostics.Process]::Start($start)
    if (-not $process.WaitForExit($Seconds * 1000)) {
        $process.Kill()
        return 'timeout'
    }
    $process.ExitCode
}
function Setup([string] $Installer, [string] $Log) {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$(Join-Path $Logs $Log)`"")
    Write-Host "+ $Installer $($arguments -join ' ')"
    Run $Installer $arguments
}
# The uninstall entries, in both registry views of both hives, that show the program's name.
function Entries {
    $roots = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    @(foreach ($root in $roots) {
        Get-ChildItem $root -ErrorAction SilentlyContinue |
            Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq $AppName } |
            ForEach-Object { $_.Name }
    })
}
function DisplayVersion {
    if (Test-Path $Key) { (Get-ItemProperty $Key).DisplayVersion } else { 'no entry' }
}
# The uninstall entry's values, one `name=value` line each, and the installed files' names.
function Snapshot {
    $lines = @()
    if (Test-Path $Key) {
        $entry = Get-Item $Key
        foreach ($name in ($entry.GetValueNames() | Sort-Object)) {
            $lines += "entry $name=$($entry.GetValue($name))"
        }
    }
    if (Test-Path $AppDir) {
        $lines += Get-ChildItem $AppDir -Recurse -File | ForEach-Object { "file $($_.FullName.Substring($AppDir.Length + 1))" } | Sort-Object
    }
    $lines
}
function StopHelpers {
    Get-Process window -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
}
function Uninstall([string] $Log) {
    StopHelpers
    $code = Run (Join-Path $AppDir 'unins000.exe') @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$(Join-Path $Logs $Log)`"")
    $returned = "folder=$(Test-Path $AppDir) entry=$(Test-Path $Key)"
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (((Test-Path $AppDir) -or (Test-Path $Key)) -and $clock.Elapsed.TotalSeconds -lt 30) {
        Start-Sleep -Milliseconds 100
    }
    Check 'silent uninstall' (($code -eq 0) -and -not (Test-Path $AppDir) -and -not (Test-Path $Key)) "exit $code; when it returned: $returned; gone $([math]::Round($clock.Elapsed.TotalSeconds, 1)) s later"
}

$helper1 = Hash $Helper1
$helper2 = Hash $Helper2
Check 'the two helper builds differ' ($helper1 -ne $helper2)

# A silent per-user install.
StopHelpers
$code = Setup $V1 'install.log'
Check 'silent install' ($code -eq 0) "exit $code"
Check "files under $AppDir" ((Test-Path (Join-Path $AppDir $Exe)) -and (Test-Path (Join-Path $AppDir 'THIRD-PARTY-LICENSES.html')))
Check 'uninstall entry in HKCU alone' (((Entries) -join ',') -eq ($Key -replace '^HKCU:', 'HKEY_CURRENT_USER')) ((Entries) -join ', ')
Check 'its version' ((DisplayVersion) -eq '0.1.0') (DisplayVersion)
Check 'a Start menu entry' (Test-Path (Join-Path ([Environment]::GetFolderPath('Programs')) "$AppName.lnk"))
Check 'no desktop entry' (-not (Test-Path (Join-Path ([Environment]::GetFolderPath('Desktop')) "$AppName.lnk")))
$inno7 = Snapshot

# An upgrade over it.
$code = Setup $V2 'upgrade.log'
Check 'upgrade' (($code -eq 0) -and ((Hash $Helper) -eq $helper2)) "exit $code"
Check 'one uninstall entry after the upgrade' ((Entries).Count -eq 1) ((Entries) -join ', ')
Check 'the new version' ((DisplayVersion) -eq '0.2.0-beta.1') (DisplayVersion)

# An upgrade while the helper shows a window: CloseApplications closes it.
$code = Setup $V1 'reinstall-window.log'
Check 'back to 0.1.0' (($code -eq 0) -and ((Hash $Helper) -eq $helper1)) "exit $code"
$process = Start-Process -FilePath $Helper -ArgumentList 'window' -PassThru
Start-Sleep -Seconds 3
Check 'the windowed helper runs' (-not $process.HasExited)
$code = Setup $V2 'upgrade-window.log'
Start-Sleep -Seconds 1
Check 'an upgrade closes the windowed helper and replaces it' (($code -eq 0) -and $process.HasExited -and ((Hash $Helper) -eq $helper2) -and ((DisplayVersion) -eq '0.2.0-beta.1')) "exit $code, helper exited=$($process.HasExited), replaced=$((Hash $Helper) -eq $helper2)"
StopHelpers

# An upgrade while the helper runs without a window: setup can't close it, so it rolls back.
$code = Setup $V1 'reinstall-hidden.log'
Check 'back to 0.1.0 again' (($code -eq 0) -and ((Hash $Helper) -eq $helper1)) "exit $code"
$process = Start-Process -FilePath $Helper -ArgumentList 'hidden' -PassThru
Start-Sleep -Seconds 3
$code = Setup $V2 'upgrade-hidden.log'
Check 'an upgrade with a windowless process running exits with 5 and keeps 0.1.0' (($code -eq 5) -and -not $process.HasExited -and ((Hash $Helper) -eq $helper1) -and ((DisplayVersion) -eq '0.1.0')) "exit $code, helper running=$(-not $process.HasExited), kept=$((Hash $Helper) -eq $helper1), version $(DisplayVersion)"
StopHelpers

# The same with a hidden top-level window that ends the helper on WM_ENDSESSION, the fix the docs
# give for a process without a window: Restart Manager closes it, and the upgrade goes through.
$process = Start-Process -FilePath $Helper -ArgumentList 'hidden-window' -PassThru
Start-Sleep -Seconds 3
Check 'the hidden-window helper runs' (-not $process.HasExited)
$code = Setup $V2 'upgrade-hidden-window.log'
Start-Sleep -Seconds 1
Check 'an upgrade ends the helper by its hidden window and replaces it' (($code -eq 0) -and $process.HasExited -and ((Hash $Helper) -eq $helper2) -and ((DisplayVersion) -eq '0.2.0-beta.1')) "exit $code, helper exited=$($process.HasExited), replaced=$((Hash $Helper) -eq $helper2)"
StopHelpers
$code = Setup $V1 'reinstall-force.log'
Check 'back to 0.1.0 before the force case' (($code -eq 0) -and ((Hash $Helper) -eq $helper1)) "exit $code"

# The same with CloseApplications=force in the custom file, which kills the windowless helper.
$process = Start-Process -FilePath $Helper -ArgumentList 'hidden' -PassThru
Start-Sleep -Seconds 3
$code = Setup $V2Force 'upgrade-hidden-force.log'
Start-Sleep -Seconds 1
Check 'with CloseApplications=force, the upgrade ends the windowless helper and replaces it' (($code -eq 0) -and $process.HasExited -and ((Hash $Helper) -eq $helper2) -and ((DisplayVersion) -eq '0.2.0-beta.1')) "exit $code, helper exited=$($process.HasExited), replaced=$((Hash $Helper) -eq $helper2)"
StopHelpers
Uninstall 'uninstall.log'

# An Inno Setup 7 upgrade over an Inno Setup 6 install of the same AppId.
$code = Setup $V1Inno6 'install-inno6.log'
Check 'silent install of the Inno Setup 6 build' ($code -eq 0) "exit $code"
$inno6 = Snapshot
$code = Setup $V2 'upgrade-inno6.log'
Check 'an Inno Setup 7 upgrade over it' (($code -eq 0) -and ((Hash $Helper) -eq $helper2)) "exit $code"
Check 'one uninstall entry after the upgrade from Inno Setup 6' ((Entries).Count -eq 1) ((Entries) -join ', ')
Check 'the new version over Inno Setup 6' ((DisplayVersion) -eq '0.2.0-beta.1') (DisplayVersion)
$upgraded = Snapshot
Uninstall 'uninstall-inno6.log'

Write-Host '===== 0.1.0 installed by Inno Setup 6, then by Inno Setup 7 (< 6 only, > 7 only)'
Compare-Object $inno6 $inno7 | ForEach-Object { "$(if ($_.SideIndicator -eq '<=') { '<' } else { '>' }) $($_.InputObject)" } | Write-Host
Write-Host '===== 0.2.0-beta.1 from Inno Setup 7 over an Inno Setup 6 install'
$upgraded | Write-Host

if ($script:Failed) { exit 1 }
