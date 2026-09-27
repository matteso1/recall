#!/usr/bin/env bash
# Recall as an app you click: a "Recall" shortcut on the Desktop and in the Start menu (search
# "Recall") that opens the canonical recall.exe. Clicking it while Recall already runs just brings
# the panel back (the exe allows one overlay at a time); the panel's x button quits. Installing
# also removes the Startup-folder auto-start (scripts/autostart-install.sh), which is optional.
# Usage: scripts/shortcut-install.sh [install|remove|status]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ACTION="${1:-install}"
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE_WIN="$WINHOME\\code\\recall-win\\overlay\\target\\swiftplay\\release\\recall.exe"
ps() { powershell.exe -NoProfile -NonInteractive -Command "$1" 2>&1 | tr -d '\r'; }
# The real Desktop and Start-menu folders (the Desktop may be redirected, e.g. to OneDrive).
DESKTOP_WIN="$(ps "[Environment]::GetFolderPath('Desktop')")"
PROGRAMS_WIN="$(ps "[Environment]::GetFolderPath('Programs')")"
LINKS=("$DESKTOP_WIN\\Recall.lnk" "$PROGRAMS_WIN\\Recall.lnk")
case "$ACTION" in
install)
    [ -f "$(wslpath -u "$EXE_WIN")" ] || { echo "not built: $EXE_WIN (run scripts/overlay-build.sh)" >&2; exit 1; }
    for link in "${LINKS[@]}"; do
        ps "\$s = (New-Object -ComObject WScript.Shell).CreateShortcut('$link'); \$s.TargetPath = '$EXE_WIN'; \$s.Arguments = ''; \$s.WorkingDirectory = '$(dirname "$EXE_WIN" | sed 's|/|\\\\|g')'; \$s.IconLocation = '$EXE_WIN,0'; \$s.Description = 'Recall: build overlay for League of Legends'; \$s.Save()"
        echo "shortcut: $link"
    done
    scripts/autostart-install.sh remove
    ;;
remove)
    for link in "${LINKS[@]}"; do
        rm -f "$(wslpath -u "$link")" && echo "removed $link"
    done
    ;;
status)
    for link in "${LINKS[@]}"; do
        [ -f "$(wslpath -u "$link")" ] && echo "present: $link" || echo "absent:  $link"
    done
    scripts/autostart-install.sh status
    ;;
*)
    echo "usage: $0 [install|remove|status]" >&2
    exit 2
    ;;
esac
