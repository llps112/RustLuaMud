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

# Command rate limiting (token bucket, anti-flood) -- matched to the server:
#   server counts each command +1, drains 40 every 2s; >60 -> struck/kicked, >20 -> minor penalty.
#   safe rule: burst_size + (commands in 2s at cmd_interval_ms) must stay <= 60 (leave headroom).
# Min gap after the burst is spent (ms): 50ms = 20/s = 40 per 2s drain cycle
cmd_interval_ms = 50
# Burst allowance at 0ms gap right after connect/idle (recommended <= 20)
burst_size = 15
# Steady refill rate (tokens/sec), should track the server drain rate (40/2s = 20/s)
cmds_per_sec = 20
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

# --- 5. Launcher (double-click to start; pins CWD to install dir) ---
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
