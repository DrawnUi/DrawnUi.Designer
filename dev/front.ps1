# Waits for a process's main window and brings it to the front (restored if minimized).
param([Parameter(Mandatory)][int]$ProcessId, [int]$TimeoutSeconds = 60)
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Front {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
}
"@
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
do {
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $proc) { return }
    $h = $proc.MainWindowHandle
    if ($h -ne [IntPtr]::Zero) {
        if ([Front]::IsIconic($h)) { [void][Front]::ShowWindow($h, 9) }   # SW_RESTORE
        [void][Front]::SetForegroundWindow($h)
        return
    }
    Start-Sleep -Milliseconds 300
} while ((Get-Date) -lt $deadline)
