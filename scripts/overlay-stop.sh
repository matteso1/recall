#!/usr/bin/env bash
# Stop the overlay on Windows.
taskkill.exe /IM recall.exe /F 2>&1 | tr -d '\r' | tail -1
# an overlay built before the rename
taskkill.exe /IM featherstorm.exe /F >/dev/null 2>&1 || true
# The auto-start watcher would bring the overlay back within seconds while the client is up.
powershell.exe -NoProfile -NonInteractive -Command 'Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and $_.Name -eq "powershell.exe" -and $_.CommandLine -like "*-File*recall-autostart.ps1*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }' >/dev/null 2>&1 || true
