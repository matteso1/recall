#!/usr/bin/env bash
# Print the on-screen rectangle (physical pixels) of every visible top-level window of a Windows
# process, to check where the overlay actually landed. Usage: scripts/win-rect.sh [process-name]
set -euo pipefail
PROC="${1:-recall}"
powershell.exe -NoProfile -NonInteractive -Command "
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public class Win {
  [DllImport(\"user32.dll\")] public static extern bool SetProcessDPIAware();
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport(\"user32.dll\")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport(\"user32.dll\")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport(\"user32.dll\")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport(\"user32.dll\")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport(\"user32.dll\", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport(\"user32.dll\")] public static extern int GetWindowLong(IntPtr h, int i);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
[Win]::SetProcessDPIAware() | Out-Null
\$ids = @(Get-Process '$PROC' -ErrorAction SilentlyContinue | ForEach-Object Id)
if (-not \$ids) { Write-Output '$PROC.exe is not running'; exit 1 }
\$rows = New-Object System.Collections.ArrayList
\$cb = [Win+EnumProc]{ param(\$h, \$l)
  [uint32]\$wpid = 0; [void][Win]::GetWindowThreadProcessId(\$h, [ref]\$wpid)
  if ((\$ids -contains [int]\$wpid) -and [Win]::IsWindowVisible(\$h)) {
    \$r = New-Object Win+RECT; [void][Win]::GetWindowRect(\$h, [ref]\$r)
    \$sb = New-Object System.Text.StringBuilder 256; [void][Win]::GetWindowText(\$h, \$sb, 256)
    \$ex = [Win]::GetWindowLong(\$h, -20)
    [void]\$rows.Add(('{0}: ''{1}'' at ({2},{3}) size {4}x{5} exstyle 0x{6:X}' -f \$wpid, \$sb.ToString(), \$r.L, \$r.T, (\$r.R - \$r.L), (\$r.B - \$r.T), \$ex))
  }
  return \$true }
[void][Win]::EnumWindows(\$cb, [IntPtr]::Zero)
if (\$rows.Count -eq 0) { Write-Output '$PROC.exe is running but has no visible window' } else { \$rows | ForEach-Object { Write-Output \$_ } }
" 2>&1 | tr -d '\r'
