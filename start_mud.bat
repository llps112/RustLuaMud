@echo off
chcp 65001 >nul
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
