@echo off
rem NOTE: do NOT chcp 65001 here. Legacy conhost stores every char as 1 cell
rem under UTF-8 codepage, which breaks CJK fullwidth rendering (glyph overlap).
rem The app writes via WriteConsoleW (Unicode API); width is decided by the
rem console codepage, so keep the system default GBK (936) on Chinese Windows.
setlocal
set NO_COLOR=1
set RUST_LOG=error
cd /d "%~dp0"

set "EXE="
if exist "RustLuaMud.exe" set "EXE=RustLuaMud.exe"
if not defined EXE if exist "target\release\RustLuaMud.exe" set "EXE=target\release\RustLuaMud.exe"

if not defined EXE (
    echo [ERROR] RustLuaMud.exe not found. Build it, or copy the exe next to this script.
    pause
    exit /b 1
)

"%EXE%" %*
pause
