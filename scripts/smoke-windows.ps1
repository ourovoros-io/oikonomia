# Smoke-test the BUNDLED Windows app: install the NSIS installer, then prove
#   - the installed app starts and draws a real window;
#   - the vault directory is in local AppData, not roaming;
#   - a second launch exits and leaves one process (single instance);
#   - closing the window hides it and keeps the app alive;
#   - launching again brings the hidden window back;
#   - running the installer with the flags the in-app update uses replaces
#     the running app and starts the new copy without asking anything.
#
# Screenshots land in target\smoke\ for a human to look at.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class SmokeWindow {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);
}
'@

$ProcessName = 'oikonomia'
$WindowTitle = 'Oikonomia'
$WaitSeconds = 90
# A blank webview is one flat colour; the real unlock screen has thousands.
$MinDistinctColours = 64
# Must stay identical to NSIS_UPDATE_ARGS in apps/desktop/src-tauri/src/update_exec.rs.
$UpdateArguments = @('/P', '/UPDATE', '/R')

$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root 'target\smoke'
New-Item -ItemType Directory -Force -Path $out | Out-Null

function Fail([string]$message) {
    Write-Error "smoke FAILED: $message"
    exit 1
}

function Get-App {
    @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
}

function Get-AppWindow {
    Get-App | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -eq $WindowTitle } |
        Select-Object -First 1
}

function Wait-Until([string]$description, [scriptblock]$condition) {
    for ($waited = 0; $waited -lt $WaitSeconds; $waited++) {
        if (& $condition) { return }
        Start-Sleep -Seconds 1
    }
    Fail "timed out waiting until $description"
}

function Save-WindowShot($process, [string]$path) {
    [SmokeWindow]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
    Start-Sleep -Seconds 1

    $rect = New-Object SmokeWindow+Rect
    [SmokeWindow]::GetWindowRect($process.MainWindowHandle, [ref]$rect) | Out-Null
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -le 0 -or $height -le 0) { Fail "window has no size ($width x $height)" }

    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)

    $colours = New-Object 'System.Collections.Generic.HashSet[int]'
    for ($x = 0; $x -lt $width; $x += 8) {
        for ($y = 0; $y -lt $height; $y += 8) {
            $colours.Add($bitmap.GetPixel($x, $y).ToArgb()) | Out-Null
        }
    }
    $graphics.Dispose()
    $bitmap.Dispose()
    $colours.Count
}

$installers = @(Get-ChildItem (Join-Path $root 'target\release\bundle\nsis\*-setup.exe'))
if ($installers.Count -ne 1) { Fail "expected one NSIS installer, found $($installers.Count)" }
$installer = $installers[0].FullName

$localVault = Join-Path $env:LOCALAPPDATA 'ourovoros\oikonomia'
$roamingVault = Join-Path $env:APPDATA 'ourovoros\oikonomia'
Remove-Item -Recurse -Force $localVault, $roamingVault -ErrorAction SilentlyContinue

Write-Host '== install'
Start-Process -FilePath $installer -ArgumentList '/S' -Wait
$installDir = Join-Path $env:LOCALAPPDATA $WindowTitle
$exe = Join-Path $installDir "$ProcessName.exe"
if (-not (Test-Path $exe)) {
    Fail "the per-user install did not put $exe in place"
}

Write-Host '== start'
Start-Process -FilePath $exe
Wait-Until 'the app shows its window' { $null -ne (Get-AppWindow) }
# Give the webview time to load and paint the first screen.
Start-Sleep -Seconds 10
$colours = Save-WindowShot (Get-AppWindow) (Join-Path $out 'windows.png')
Write-Host "window drawn with $colours distinct colours"
if ($colours -lt $MinDistinctColours) {
    Fail "the app drew a blank window ($colours colours); see target\smoke\windows.png"
}

Write-Host '== vault location'
if (-not (Test-Path $localVault)) { Fail "no vault directory at $localVault" }
if (Test-Path $roamingVault) { Fail "the vault directory was created in roaming AppData: $roamingVault" }

Write-Host '== single instance'
$second = Start-Process -FilePath $exe -PassThru
if (-not $second.WaitForExit(30000)) { Fail 'a second launch kept running' }
$count = (Get-App).Count
if ($count -ne 1) { Fail "expected one process after a second launch, found $count" }

Write-Host '== close hides, relaunch shows'
(Get-AppWindow).CloseMainWindow() | Out-Null
Wait-Until 'the window is hidden after close' { $null -eq (Get-AppWindow) }
if ((Get-App).Count -ne 1) { Fail 'the app quit when its window was closed' }

$reopen = Start-Process -FilePath $exe -PassThru
if (-not $reopen.WaitForExit(30000)) { Fail 'the reopening launch kept running' }
Wait-Until 'the hidden window is shown again' { $null -ne (Get-AppWindow) }
if ((Get-App).Count -ne 1) { Fail 'reopening left more than one process' }

Write-Host '== update handoff'
$before = (Get-App)[0].Id
Start-Process -FilePath $installer -ArgumentList $UpdateArguments -Wait
Wait-Until 'the installer starts the new copy' {
    $window = Get-AppWindow
    $null -ne $window -and $window.Id -ne $before
}
if ((Get-App).Count -ne 1) { Fail 'the update left more than one process' }
Start-Sleep -Seconds 8
Save-WindowShot (Get-AppWindow) (Join-Path $out 'windows-after-update.png') | Out-Null

Get-App | Stop-Process -Force
Write-Host 'smoke ok: the installed app starts, draws, stays single, comes back from hidden, and updates in place'
