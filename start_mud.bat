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

rem Prefer the exe next to this script, fall back to the cargo build output.
set "EXE="
if exist "RustLuaMud.exe" set "EXE=RustLuaMud.exe"
if not defined EXE if exist "target\release\RustLuaMud.exe" set "EXE=target\release\RustLuaMud.exe"

if not defined EXE (
    echo [ERROR] RustLuaMud.exe not found. Build it, or copy the exe next to this script.
    pause
    exit /b 1
)

"%EXE%" %*
if errorlevel 1 pause
