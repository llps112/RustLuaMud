@echo off
rem Do NOT chcp 65001: legacy conhost stores every char as 1 cell under the
rem UTF-8 codepage, breaking CJK fullwidth rendering (glyph overlap).
rem Keep system default (GBK 936 on Chinese Windows); the app writes via the
rem Unicode console API, only the width rule depends on the codepage.
setlocal
set NO_COLOR=1
set RUST_LOG=error
cd /d "%~dp0"

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
