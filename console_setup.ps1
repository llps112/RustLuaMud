# ============================================
# RustLuaMud conhost geometry setup
#
# Resizes and centers the legacy console window before RustLuaMud.exe starts:
#   * target 160x56 cells, clamped to whatever the screen actually fits at the
#     current console font -- J1800 boxes often drive 1024x768 monitors, where
#     160 columns (about 1280 px) simply do not fit
#   * screen buffer is set equal to the window, because a taller buffer makes
#     conhost show a vertical scrollbar that overlays the floating panel and
#     the right-aligned status bar
#   * window is centered on the primary screen's working area (taskbar excluded)
#
# Exits 0 without touching anything when there is no real conhost window to
# manage: under Windows Terminal, when output is redirected, or in a service
# context. A geometry failure is never a reason to skip the launch.
#
# Called from start_mud.bat. Override the target with MUD_COLS / MUD_LINES.
#
# ASCII-only source on purpose: Windows PowerShell 5.1 may misdecode non-ASCII
# UTF-8 without BOM (same policy as deploy.ps1 / bootstrap.ps1). The C# block
# below uses a multi-line single-quoted string, not a here-string, so that
# bootstrap.ps1 can embed this file verbatim inside its own here-string.
# ============================================

param(
    [int]$Cols = 160,
    [int]$Rows = 56
)

if ($env:MUD_COLS -match '^\d+$') { $Cols = [int]$env:MUD_COLS }
if ($env:MUD_LINES -match '^\d+$') { $Rows = [int]$env:MUD_LINES }

$ErrorActionPreference = 'SilentlyContinue'

# Windows Terminal hosts the console itself: GetConsoleWindow() returns a pseudo
# handle and buffer resizing is rejected. Leave geometry to WT's own settings.
if ($env:WT_SESSION) { exit 0 }

Add-Type -TypeDefinition '
using System;
using System.Runtime.InteropServices;
public static class ConWin {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("kernel32.dll")]
    public static extern IntPtr GetConsoleWindow();
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")]
    public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int w, int h, bool repaint);
    [DllImport("user32.dll")]
    public static extern bool SystemParametersInfo(int action, int param, ref RECT area, int ini);
}
'

if (-not ('ConWin' -as [type])) { exit 0 }

$hwnd = [ConWin]::GetConsoleWindow()
if ($hwnd -eq [IntPtr]::Zero) { exit 0 }

$raw = $Host.UI.RawUI

# MaxPhysicalWindowSize is reported in character cells -- the same unit
# WindowSize uses -- so it is exactly the clamp we need. Implausible values
# mean the metrics did not come from a real conhost; bail out rather than
# resize the window into something unusable.
$max = $raw.MaxPhysicalWindowSize
if ($max.Width -lt 20 -or $max.Width -gt 500) { exit 0 }
if ($max.Height -lt 5 -or $max.Height -gt 200) { exit 0 }
if ($Cols -gt $max.Width) { $Cols = $max.Width }
if ($Rows -gt $max.Height) { $Rows = $max.Height }

# The only resize order that cannot fail: the window may never be wider than
# the buffer, so grow the buffer first, then set the window, then shrink the
# buffer back onto the window to drop the scrollbar.
$cur = $raw.BufferSize
$grownW = [Math]::Max($Cols, $cur.Width)
$grownH = [Math]::Max($Rows, $cur.Height)
$grown = New-Object System.Management.Automation.Host.Size -ArgumentList $grownW, $grownH
$raw.BufferSize = $grown
$target = New-Object System.Management.Automation.Host.Size -ArgumentList $Cols, $Rows
$raw.WindowSize = $target
$raw.BufferSize = $target

# Center via SPI_GETWORKAREA (0x30) instead of System.Windows.Forms: this runs
# on every launch, and loading WinForms costs more than the whole script on
# J1800-class hardware.
$rect = New-Object ConWin+RECT
if (-not [ConWin]::GetWindowRect($hwnd, [ref]$rect)) { exit 0 }
$work = New-Object ConWin+RECT
if (-not [ConWin]::SystemParametersInfo(0x0030, 0, [ref]$work, 0)) { exit 0 }

$winW = $rect.Right - $rect.Left
$winH = $rect.Bottom - $rect.Top
$x = $work.Left + [int](($work.Right - $work.Left - $winW) / 2)
$y = $work.Top + [int](($work.Bottom - $work.Top - $winH) / 2)
if ($x -lt $work.Left) { $x = $work.Left }
if ($y -lt $work.Top) { $y = $work.Top }

[ConWin]::MoveWindow($hwnd, $x, $y, $winW, $winH, $true) | Out-Null
exit 0
