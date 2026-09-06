#!/usr/bin/env bash
# Tail the overlay log (%LOCALAPPDATA%\Recall\recall.log) through PowerShell, because
# folders created from Windows can stay invisible to /mnt/c for a while. Usage: scripts/overlay-log.sh [lines]
N="${1:-40}"
powershell.exe -NoProfile -NonInteractive -Command "Get-Content -Tail $N \"\$env:LOCALAPPDATA\Recall\recall.log\"" 2>&1 | tr -d '\r'
