#!/usr/bin/env bash
# Install (or remove) Recall's auto-start: a shortcut in the user's Startup folder that runs the
# canonical `recall.exe --autostart`. In that mode the overlay itself stays hidden until the League
# client is running, shows while it is, and hides again 20 s after the client closes. No admin
# rights, no scripts, no terminal window. Also starts it right away.
# Usage: scripts/autostart-install.sh [install|remove|status]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ACTION="${1:-install}"
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE_WIN="$WINHOME\\code\\recall-win\\overlay\\target\\swiftplay\\release\\recall.exe"
STARTUP_WIN="$WINHOME\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup"
LINK_WIN="$STARTUP_WIN\\Recall.lnk"
LINK="$(wslpath -u "$LINK_WIN")"
ps() { powershell.exe -NoProfile -NonInteractive -Command "$1" 2>&1 | tr -d '\r'; }
case "$ACTION" in
install)
    [ -f "$(wslpath -u "$EXE_WIN")" ] || { echo "not built: $EXE_WIN (run scripts/overlay-build.sh)" >&2; exit 1; }
    ps "\$s = (New-Object -ComObject WScript.Shell).CreateShortcut('$LINK_WIN'); \$s.TargetPath = '$EXE_WIN'; \$s.Arguments = '--autostart'; \$s.WorkingDirectory = '$(dirname "$EXE_WIN" | sed 's|/|\\\\|g')'; \$s.Description = 'Recall: build overlay for League of Legends'; \$s.Save()"
    # Leftovers of the earlier PowerShell watcher.
    ps 'Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and $_.Name -eq "powershell.exe" -and $_.CommandLine -like "*-File*recall-autostart.ps1*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }' >/dev/null || true
    rm -f "$(wslpath -u "$STARTUP_WIN")/Recall auto-start.lnk" "$(wslpath -u "$WINHOME")/AppData/Local/Recall/recall-autostart.ps1"
    if ! tasklist.exe 2>/dev/null | tr -d '\r' | grep -q "^recall.exe"; then
        nohup cmd.exe /c start "" "$LINK_WIN" >/dev/null 2>&1 </dev/null &
        sleep 2
    fi
    echo "installed: $LINK_WIN"
    ;;
remove)
    rm -f "$LINK"
    echo "removed $LINK_WIN (a running overlay is left alone)"
    ;;
status)
    [ -f "$LINK" ] && echo "startup shortcut: present" || echo "startup shortcut: absent"
    echo "overlay processes: $(tasklist.exe 2>/dev/null | tr -d '\r' | grep -c '^recall.exe' || true)"
    ;;
*)
    echo "usage: $0 [install|remove|status]" >&2
    exit 2
    ;;
esac
