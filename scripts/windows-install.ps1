<#
.SYNOPSIS
  One-click Windows helper for Cymesh:
  1. Copies Cymesh.exe into %LOCALAPPDATA%\CymeshOpenSource (no Admin required)
  2. Creates Desktop and Start Menu shortcuts
  3. Optionally adds Windows Firewall rules for Cymesh mesh ports
#>
param(
    [string]$ExePath = "",
    [switch]$AddFirewallRules
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $Candidates = @(
        (Join-Path $PSScriptRoot "Cymesh.exe"),
        (Join-Path $PSScriptRoot "..\dist-windows\Cymesh.exe"),
        (Join-Path $PSScriptRoot "..\Cymesh.exe"),
        (Join-Path $PSScriptRoot "..\target\release\Cymesh.exe"),
        (Join-Path $PSScriptRoot "..\target\x86_64-pc-windows-gnu\release\aegis-mesh-app.exe")
    )
    foreach ($C in $Candidates) {
        if (Test-Path $C) {
            $ExePath = (Resolve-Path $C).Path
            break
        }
    }
}

if ([string]::IsNullOrWhiteSpace($ExePath) -or -not (Test-Path $ExePath)) {
    Write-Host "Could not find Cymesh.exe next to this script or in dist-windows." -ForegroundColor Red
    exit 1
}

$ExePath = (Resolve-Path -LiteralPath $ExePath).Path
$SourceDir = Split-Path -Parent $ExePath
$InstallDir = Join-Path $env:LOCALAPPDATA "CymeshOpenSource"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$TargetExe = Join-Path $InstallDir "Cymesh.exe"
if ($ExePath -ne $TargetExe) {
    Copy-Item -LiteralPath $ExePath -Destination $TargetExe -Force
}

$LoaderDll = Join-Path $SourceDir "WebView2Loader.dll"
$TargetLoaderDll = Join-Path $InstallDir "WebView2Loader.dll"
if ($LoaderDll -ne $TargetLoaderDll -and (Test-Path -LiteralPath $LoaderDll)) {
    Copy-Item -LiteralPath $LoaderDll -Destination $TargetLoaderDll -Force
}

$WshShell = New-Object -ComObject WScript.Shell
$DesktopLnk = Join-Path ([Environment]::GetFolderPath("Desktop")) "Cymesh Open Source.lnk"
$StartMenuLnk = Join-Path ([Environment]::GetFolderPath("Programs")) "Cymesh Open Source.lnk"

foreach ($LnkPath in @($DesktopLnk, $StartMenuLnk)) {
    $Shortcut = $WshShell.CreateShortcut($LnkPath)
    $Shortcut.TargetPath = $TargetExe
    $Shortcut.WorkingDirectory = $InstallDir
    $Shortcut.Description = "Cymesh - Zero-Server End-to-End Encrypted Messenger"
    $Shortcut.Save()
}

# Invalidate Windows shell icon cache so new icon appears immediately
try {
    $code = @"
    using System;
    using System.Runtime.InteropServices;
    public class ShellIcon {
        [DllImport("shell32.dll", SetLastError = true)]
        public static extern void SHChangeNotify(int wEventId, int uFlags, IntPtr dwItem1, IntPtr dwItem2);
    }
"@
    Add-Type -TypeDefinition $code
    [ShellIcon]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
} catch {}

Write-Host "Cymesh installed cleanly to $TargetExe" -ForegroundColor Cyan
Write-Host "Desktop & Start Menu shortcuts created!" -ForegroundColor Green

if ($AddFirewallRules) {
    try {
        New-NetFirewallRule -DisplayName "Cymesh Mesh Discovery (UDP 47832)" -Direction Inbound -Protocol UDP -LocalPort 47832 -Action Allow -Profile Private,Public -ErrorAction SilentlyContinue | Out-Null
        New-NetFirewallRule -DisplayName "Cymesh Noise P2P (TCP 47833)" -Direction Inbound -Protocol TCP -LocalPort 47833 -Action Allow -Profile Private,Public -ErrorAction SilentlyContinue | Out-Null
        New-NetFirewallRule -DisplayName "Cymesh Direct UDP / NAT Punch (UDP 47833)" -Direction Inbound -Protocol UDP -LocalPort 47833 -Action Allow -Profile Private,Public -ErrorAction SilentlyContinue | Out-Null
        Write-Host "Windows Firewall rules (47832 UDP, 47833 TCP/UDP) configured." -ForegroundColor Green
    } catch {
        Write-Host "Note: Run as Administrator if you want automatic Windows Firewall rules." -ForegroundColor Yellow
    }
}
