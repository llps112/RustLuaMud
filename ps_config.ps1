# ============================================
# RustLuaMud PowerShell console configuration
# Black background + white foreground, fixed window size, buffer == window.
#
# Scope: the everyday PowerShell window only. RustLuaMud's own window is sized
# by console_setup.ps1 (160x56, clamped to the screen and centered), which
# start_mud.bat calls before launching the exe -- running the .bat from here
# hands the window over to it, so the two need no matching numbers.
#
# buffer == window also drops scroll-back for long output (cargo test, git log).
# That trade-off came from the MUD, where a scrollbar can cover the floating
# panel; PowerShell has no such constraint, so raise the BufferSize below if
# you need scroll-back.
#
# Usage: dot-source from your $PROFILE, or run:  . .\ps_config.ps1
# ============================================

# 1. Colors
try {
    $raw = $Host.UI.RawUI
    $raw.BackgroundColor = [ConsoleColor]::Black
    $raw.ForegroundColor = [ConsoleColor]::White
    Write-Host "[OK] console colors: black background, white foreground"
} catch {
    Write-Host "[WARN] cannot set console colors: $_" -ForegroundColor Yellow
}

# 2. Window + buffer size (buffer set equal to window to drop the scrollbar)
try {
    $raw = $Host.UI.RawUI
    $ws = $raw.WindowSize
    $ws.Width = 120
    $ws.Height = 30
    $raw.WindowSize = $ws

    $bs = $raw.BufferSize
    $bs.Width = 120
    $bs.Height = 30
    $raw.BufferSize = $bs

    Write-Host "[OK] window & buffer set to 120x30 (buffer==window removes the scrollbar)"
} catch {
    Write-Host "[WARN] cannot resize console: $_" -ForegroundColor Yellow
}

# 3. Helper + alias: list the 5 most recent MUD log files
function Get-RlmLog {
    param([string]$LogDir = (Join-Path $HOME 'RustLuaMud\logs'))
    Get-ChildItem -Path (Join-Path $LogDir '*.log') -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 5 Name, LastWriteTime
}
Set-Alias rlmc Get-RlmLog
