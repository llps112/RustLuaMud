#!/usr/bin/env pwsh
# RustLuaMud one-click bootstrap (Windows)
# Creates a data directory (default: %USERPROFILE%\RustLuaMud), downloads the
# prebuilt Windows binary and generates example config/scripts.
#
# Usage:
#   .\bootstrap.ps1                    # stable release
#   .\bootstrap.ps1 -Nightly           # nightly build
#   .\bootstrap.ps1 -Gitee             # use Gitee mirror (mainland China)
#   .\bootstrap.ps1 -Nightly -Gitee
#   .\bootstrap.ps1 D:\Games\RustLuaMud  # custom install directory
#
# Notes:
#   - This ASCII-only script avoids PowerShell 5.1 non-ASCII decoding issues.
#   - Game scripts are NOT included here; drop your own scripts into scripts\.

param(
    [Parameter(Position = 0)]
    [string]$Target,
    [switch]$Nightly,
    [switch]$Gitee
)

$ErrorActionPreference = "Stop"

# --- Config ---
if (-not $Target) {
    $Target = Join-Path $env:USERPROFILE "RustLuaMud"
}
$GhOwner = "llps112";      $GhRepo = "RustLuaMud"
$GtOwner = "bai-yifei180"; $GtRepo = "RustLuaMud"
$Asset   = "RustLuaMud-windows-x86_64.zip"

# --- Resolve download URL ---
if ($Gitee) {
    if ($Nightly) {
        $Url   = "https://gitee.com/$GtOwner/$GtRepo/releases/download/nightly/$Asset"
        $Label = "nightly (Gitee)"
    }
    else {
        $latest = $null
        try {
            $rel  = Invoke-RestMethod "https://gitee.com/api/v5/repos/$GtOwner/$GtRepo/releases?per_page=100"
            # Only strict semantic tags (vX.Y.Z) are sortable by [version];
            # prerelease tags like v1.0.0-beta would throw and fall back to nightly.
            $tags = $rel | Where-Object { $_.tag_name -match '^v\d+(\.\d+){1,3}$' } |
                ForEach-Object { $_.tag_name }
            $latest = $tags | Sort-Object { [version]($_ -replace '^v', '') } | Select-Object -Last 1
        }
        catch { $latest = $null }

        if ($latest) {
            $Url   = "https://gitee.com/$GtOwner/$GtRepo/releases/download/$latest/$Asset"
            $Label = "stable ($latest, Gitee)"
        }
        else {
            $Url   = "https://gitee.com/$GtOwner/$GtRepo/releases/download/nightly/$Asset"
            $Label = "nightly (Gitee fallback)"
        }
    }
}
else {
    if ($Nightly) {
        $Url   = "https://github.com/$GhOwner/$GhRepo/releases/download/nightly/$Asset"
        $Label = "nightly"
    }
    else {
        $Url   = "https://github.com/$GhOwner/$GhRepo/releases/latest/download/$Asset"
        $Label = "stable"
    }
}

Write-Host "=========================================="
Write-Host "  RustLuaMud Bootstrap (Windows)"
Write-Host "  Channel : $Label"
Write-Host "  Target  : $Target"
Write-Host "=========================================="
Write-Host ""

# --- 1. Create data directory ---
foreach ($d in @("", "profiles", "scripts", "logs")) {
    $p = if ($d) { Join-Path $Target $d } else { $Target }
    New-Item -ItemType Directory -Force -Path $p | Out-Null
}
Write-Host "==> Data directory ready: $Target"

# --- 2. Download and unpack binary ---
$Zip = Join-Path $env:TEMP ("rlm-{0}.zip" -f [Guid]::NewGuid().ToString('N').Substring(0, 8))
Write-Host "==> Downloading binary..."
Write-Host "    $Url"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing
}
catch {
    Write-Host "!! Download failed: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "   Check the network, or confirm this release actually ships a Windows artifact." -ForegroundColor Yellow
    Remove-Item $Zip -ErrorAction SilentlyContinue
    exit 1
}

$exeTarget = Join-Path $Target "RustLuaMud.exe"
if (Test-Path $exeTarget) { Remove-Item $exeTarget -Force }
Expand-Archive -Path $Zip -DestinationPath $Target -Force
Remove-Item $Zip -ErrorAction SilentlyContinue
Write-Host "    [OK] Unpacked RustLuaMud.exe" -ForegroundColor Green

# --- 3. Example role config (skip if exists) ---
$exampleToml = Join-Path $Target "profiles\example.toml"
if (-not (Test-Path $exampleToml)) {
    Write-Host "==> Creating example config: $exampleToml"
    @'
# Role connection config
# The file name is the role identity; recommend naming it after your character.
#
# After adding this file at runtime you can load it inside the client (no restart):
#   /profile list          - list available roles
#   /profile load <name>   - load and connect

# Connection info
name = "your_character_name"
host = "ln.xkxmud.com"
port = 5555
encoding = "gbk"

# Lua script path (relative to the program run directory)
script = "scripts/example.lua"

# Connection behavior
auto_connect = true
auto_reconnect = true
reconnect_delay_secs = 5

# Login credentials (auto-injected into Lua vars char_name / char_password at startup)
# Leave empty to skip injection (type manually or set via Lua setname/setpwd).
#
# To keep the password OUT of this file (so copying/sharing the TOML never leaks it),
# use an environment-variable placeholder -- a value that is exactly ${NAME} is read
# from the environment at startup:
#   password = "${MUD_MYCHAR_PWD}"
#   Set it once on Windows: setx MUD_MYCHAR_PWD "real_password"   (takes effect in a NEW terminal)
#   If the variable is missing, the field is treated as empty and a startup warning is
#   printed; the placeholder text is NEVER sent to the server as the password.
#   If the password itself literally looks like ${XXX}, escape the $: "$${LITERAL}".
#
# To manage several roles' passwords in one place instead of setx, put the variables in
# profiles\.env (see profiles\.env.example) and reference them the same way.
username = "your_character_name"
password = "your_password"

# SOCKS5 proxy (optional; direct connection when disabled)
socks5_enable = false
socks5_host = "127.0.0.1"
socks5_port = 1080
socks5_username = ""
socks5_password = ""

# Realtime rendering (optional; when true, render_interval is ignored)
realtime = true
# Render interval in ms (0 = realtime, default 1000 = refresh once per second)
render_interval = 1000

# Log files kept (optional; default 24 = last 24 hourly log files)
log_rotation_count = 24

# Command rate limiting (token bucket + sliding window, optional)
#   Rate limiting is enforced on the Rust side; Lua scripts only enqueue.
#
# Server-side mechanism (LPC cmd.c):
#   - cnt counts every command (+1); every 2s it drains 40 (clear_cmd_count)
#   - cnt > 60 -> struck/unconscious/kicked;  cnt > 20 -> minor penalty
#   Equivalent token bucket: capacity 60, refilled 40 every 2s.
#
# Safety inequalities (BOTH must hold, otherwise long idle sessions still get struck):
#   1) cmds_per_sec <= 20                 - long-term rate must not exceed the drain rate
#   2) burst_size + 2*cmds_per_sec <= 60  - one burst plus 2s of steady traffic
#   e.g. burst=15, cmds_per_sec=20 -> 15 + 40 = 55 <= 60, leaving 5 tokens of headroom.
#   Note: cmd_interval_ms is NOT a long-term rate cap - it only spaces out non-burst
#   sends; surplus tokens accumulate up to burst_size and are then spent in a burst.
#   The long-term rate is set by cmds_per_sec. Both inequalities are validated when the
#   config is parsed; a warning is printed at startup and on /profile load if violated.
#
# Min gap after the burst is spent (ms, default 50, range 20~200)
#   50ms = 20/s = 40 per 2s drain cycle
cmd_interval_ms = 50
#
# Burst allowance at 0ms gap right after connect/idle (default 10;
# must satisfy burst_size + 2*cmds_per_sec <= 60)
burst_size = 15
#
# Steady refill rate (tokens/sec, default 20) - tracks the server drain rate (40/2s)
#   Never raise it above 20: cnt then grows every cycle and window_limit = 60 cannot
#   stop this kind of long-term overspeed
cmds_per_sec = 20
#
# Max commands allowed inside the sliding window (default 60, range 1~1000)
#   Matches the server strike threshold 3*CMDS_PER_TICK; does not rely on being
#   aligned with the server tick. It caps burst DENSITY (half-open interval), not the
#   long-term rate. With 60 the two inequalities above still have to hold; to cap
#   unconditionally set 40 (= what the server drains per cycle), which keeps cnt <= 40
#   even if the token bucket is misconfigured, at the cost of burst throughput.
window_limit = 60
#
# Sliding window duration (ms, default 2000, range 2000~10000)
#   Matches the server's 2s clear_cmd_count drain period; usually leave as is.
#   Must not go below 2000: shorter windows make the fallback ineffective, the runtime
#   raises it back to 2000 and prints a warning.
window_duration_ms = 2000
'@ | Set-Content -Path $exampleToml -Encoding ASCII
}

# --- 3b. Example credential file (.env.example; skip if exists) ---
$envExample = Join-Path $Target "profiles\.env.example"
if (-not (Test-Path $envExample)) {
    Write-Host "==> Creating example env file: $envExample"
    @'
# ============================================================
# RustLuaMud credential file example (.env)
# ============================================================
# Keep all passwords in this ONE file and reference them from role configs
# (*.toml) with a placeholder like "${VAR_NAME}". Copying or sharing the TOML
# files then never carries your real passwords.
#
# Steps:
#   1. Copy this file to ".env" in the same folder (from cmd):
#        copy .env.example .env
#   2. Edit .env, one "VAR_NAME=value" per line:
#        MUD_GBDOOR_PWD=my_real_password
#   3. Reference it in the role config (e.g. gbdoor.toml):
#        password = "${MUD_GBDOOR_PWD}"
#   4. Start the client; the password is read from .env at login.
#
# Format rules:
#   - One entry per line: NAME=value  (spaces around = are trimmed)
#   - NAME must start with a letter or underscore; letters/digits/underscore only
#   - Lines starting with # are comments; blank lines are ignored
#   - Quote values that contain spaces: MUD_PWD="my pass word"  (quotes are stripped)
#
# Notes:
#   - Save as UTF-8 (Notepad: Save As -> Encoding UTF-8). ANSI/GBK causes the whole
#     file to fail loading and prints a startup warning.
#   - .env is git-ignored; never commit or share it.
#   - Real environment variables (e.g. set via setx) take precedence over .env.
#   - Restart the client after editing .env for changes to take effect.
#   - A missing/misspelled variable yields an empty value plus a startup warning; the
#     placeholder text is never sent to the server.
# ============================================================
# Example entries -- copy to .env and edit:
# ============================================================

# gbdoor role login password
MUD_GBDOOR_PWD=replace_with_real_password

# a second role (name is arbitrary; must match ${...} used in the toml)
MUD_FKAKMA_PWD=replace_with_real_password

# usernames and SOCKS5 passwords support placeholders too:
# MUD_GBDOOR_USER=gbdoor
# MUD_SOCKS5_PWD=proxy_password
'@ | Set-Content -Path $envExample -Encoding ASCII
}

# --- 4. Example Lua script (skip if exists) ---
$exampleLua = Join-Path $Target "scripts\example.lua"
if (-not (Test-Path $exampleLua)) {
    Write-Host "==> Creating example script: $exampleLua"
    @'
-- RustLuaMud example script
trigger("Are you using BIG5 code\?", function()
    send("No")
    Note("answered BIG5 prompt")
end)

alias("^lh$", function() send("look"); send("hp") end)
alias("^gs$", function() send("go south") end)
alias("^gn$", function() send("go north") end)
alias("^gw$", function() send("go west") end)
alias("^ge$", function() send("go east") end)

timer(60, function() send("hp") end)
Note("example.lua loaded")
'@ | Set-Content -Path $exampleLua -Encoding ASCII
}

# --- 5. Console geometry helper + launcher (double-click to start) ---
# Sizing AND centering the conhost window needs Win32 calls cmd cannot make, so
# the launcher delegates to this helper. Keep this copy in sync with the
# repo-root console_setup.ps1 (the launcher expects it beside itself).
# The inner C# block is a multi-line single-quoted string rather than a
# here-string on purpose: a here-string terminator at the start of a line would
# close this outer here-string early.
$setup = Join-Path $Target "console_setup.ps1"
Write-Host "==> Creating console geometry helper: $setup"
@'
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
'@ | Set-Content -Path $setup -Encoding ASCII

$bat = Join-Path $Target "start_mud.bat"
Write-Host "==> Creating launcher: $bat"
@'
@echo off
rem Do NOT chcp 65001: legacy conhost stores every char as 1 cell under the
rem UTF-8 codepage, breaking CJK fullwidth rendering (glyph overlap).
rem Keep system default (GBK 936 on Chinese Windows); the app writes via the
rem Unicode console API, only the width rule depends on the codepage.
setlocal
set NO_COLOR=1
set RUST_LOG=error
cd /d "%~dp0"

rem Console geometry: 160x56 cells by default, clamped to what the screen fits
rem at the current console font, and centered on the working area. Override by
rem setting MUD_COLS / MUD_LINES before launching. console_setup.ps1 no-ops
rem under Windows Terminal, which owns its own geometry.
rem A missing helper or an unavailable PowerShell is not fatal: an unresized
rem window still works, so never block the launch on cosmetics.
if not defined MUD_COLS set "MUD_COLS=160"
if not defined MUD_LINES set "MUD_LINES=56"
set "PS=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
set "SETUP=%~dp0console_setup.ps1"
if exist "%SETUP%" if exist "%PS%" (
    "%PS%" -NoProfile -ExecutionPolicy Bypass -File "%SETUP%"
)

RustLuaMud.exe %*
if errorlevel 1 pause
'@ | Set-Content -Path $bat -Encoding ASCII

# --- 6. Done ---
Write-Host ""
Write-Host "==========================================" -ForegroundColor Green
Write-Host "  RustLuaMud is ready" -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Green
Write-Host ""
Write-Host "  Directory layout:"
Write-Host "    $Target\"
Write-Host "      RustLuaMud.exe        <- main program"
Write-Host "      start_mud.bat         <- double-click to launch"
Write-Host "      console_setup.ps1     <- sizes/centers the window (called by the .bat)"
Write-Host "      profiles\             <- role TOML configs (terminal.json lives here)"
Write-Host "        example.toml        <- example config"
Write-Host "        .env.example        <- copy to .env to keep passwords out of the TOMLs"
Write-Host "      scripts\              <- put your game scripts here"
Write-Host "        example.lua"
Write-Host "      logs\                 <- generated at runtime"
Write-Host ""
Write-Host "  First run:"
Write-Host "    1. Copy the example config:"
Write-Host "       copy `"$Target\profiles\example.toml`" `"$Target\profiles\mychar.toml`""
Write-Host "    2. Edit profiles\mychar.toml (host / account / password / script path)"
Write-Host "    3. Double-click start_mud.bat, or run:"
Write-Host "       `"$Target\RustLuaMud.exe`""
Write-Host ""
Write-Host "  Tip: on Windows run inside Windows Terminal (recommended) for correct"
Write-Host "       ANSI colors, CJK alignment and floating panels."
Write-Host ""
