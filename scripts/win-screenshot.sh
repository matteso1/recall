#!/usr/bin/env bash
# Capture the Windows primary screen (full DPI) to a PNG inside the WSL filesystem, so the
# overlay can be inspected from WSL. Usage: scripts/win-screenshot.sh [out.png] [x,y,w,h]
# The optional crop (physical pixels) keeps the file small when only the panel matters.
# (Writes through \\wsl.localhost: folders created from Windows can be invisible to /mnt/c for a while.)
set -euo pipefail
OUT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.screens/screenshot.png}"
CROP="${2:-}"
mkdir -p "$(dirname "$OUT")"
OUT_WIN="$(wslpath -w "$OUT")"
powershell.exe -NoProfile -NonInteractive -Command "
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public class DpiFix { [DllImport(\"user32.dll\")] public static extern bool SetProcessDPIAware(); }'
[DpiFix]::SetProcessDPIAware() | Out-Null
\$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
\$bmp = New-Object System.Drawing.Bitmap \$b.Width, \$b.Height
\$g = [System.Drawing.Graphics]::FromImage(\$bmp)
\$g.CopyFromScreen(\$b.Location, [System.Drawing.Point]::Empty, \$b.Size)
\$out = \$bmp
if ('$CROP' -ne '') {
  \$c = '$CROP'.Split(',') | ForEach-Object { [int]\$_ }
  \$rect = New-Object System.Drawing.Rectangle \$c[0], \$c[1], \$c[2], \$c[3]
  \$rect.Intersect((New-Object System.Drawing.Rectangle 0, 0, \$b.Width, \$b.Height))
  \$out = \$bmp.Clone(\$rect, \$bmp.PixelFormat)
}
\$out.Save('$OUT_WIN', [System.Drawing.Imaging.ImageFormat]::Png)
\$g.Dispose(); \$bmp.Dispose()
Write-Output ('captured ' + \$b.Width + 'x' + \$b.Height + ' -> ' + \$out.Width + 'x' + \$out.Height)
" | tr -d '\r'
echo "$OUT"
