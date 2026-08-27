# ============================================
# RustLuaMud PowerShell console configuration
# Black background + white foreground, fixed window size, and buffer==window
# (buffer height equal to window height removes the conhost vertical scrollbar
#  that otherwise overlays the floating panel / right-aligned status logo).
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
