#!/usr/bin/env bash
# Install (or remove) the Recall auto-start watcher on Windows: a hidden PowerShell loop that starts
# the overlay whenever the League client is running and closes it when the client closes.
# No admin rights: the script is copied to %LOCALAPPDATA%\Recall and a shortcut goes into the
# user's Startup folder. Also starts the watcher right away.
# Usage: scripts/autostart-install.sh [install|remove|status]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ACTION="${1:-install}"
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
LOCALAPP="$(cmd.exe /c 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')"
STARTUP_WIN="$WINHOME\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup"
SCRIPT_WIN="$LOCALAPP\\Recall\\recall-autostart.ps1"
LINK_WIN="$STARTUP_WIN\\Recall auto-start.lnk"
ps() { powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$1" 2>&1 | tr -d '\r'; }
# The query excludes itself (its own command line names the script).
WATCHERS='Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and $_.Name -eq "powershell.exe" -and $_.CommandLine -like "*-File*recall-autostart.ps1*" }'
watcher_running() {
    ps "$WATCHERS | Measure-Object | Select-Object -ExpandProperty Count"
}
case "$ACTION" in
install)
    mkdir -p "$(wslpath -u "$LOCALAPP")/Recall"
    cp scripts/windows/recall-autostart.ps1 "$(wslpath -u "$SCRIPT_WIN")"
    ps "\$s = (New-Object -ComObject WScript.Shell).CreateShortcut('$LINK_WIN'); \$s.TargetPath = 'powershell.exe'; \$s.Arguments = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File \"$SCRIPT_WIN\"'; \$s.WorkingDirectory = '$LOCALAPP\\Recall'; \$s.Description = 'Start Recall with the League client'; \$s.Save()"
    if [ "$(watcher_running)" = "0" ]; then
        # Start it the same way logon will: through the shortcut.
        nohup cmd.exe /c start "" "$LINK_WIN" >/dev/null 2>&1 </dev/null &
        sleep 3
    fi
    echo "installed: $LINK_WIN"
    echo "watcher processes: $(watcher_running)"
    ;;
remove)
    ps "$WATCHERS"' | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }'
    rm -f "$(wslpath -u "$LINK_WIN")" "$(wslpath -u "$SCRIPT_WIN")"
    echo "removed"
    ;;
status)
    [ -f "$(wslpath -u "$LINK_WIN")" ] && echo "startup shortcut: present" || echo "startup shortcut: absent"
    echo "watcher processes: $(watcher_running)"
    tail -n 5 "$(wslpath -u "$LOCALAPP")/Recall/autostart.log" 2>/dev/null || true
    ;;
*)
    echo "usage: $0 [install|remove|status]" >&2
    exit 2
    ;;
esac
