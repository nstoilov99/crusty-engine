# Capture a window by title substring to a PNG.
# Usage: pwsh screenshot.ps1 -TitleMatch "Rust Game Engine" -Out ref.png
param(
    [string]$TitleMatch = "Rust Game Engine",
    [string]$Out = "shot.png",
    [switch]$KeepCursor  # skip cursor parking (e.g. mid-drag captures)
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class Win32Cap {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    // Get-Process MainWindowTitle/Handle track whichever window has focus,
    // so they misfire when the app has multiple windows (dock floats).
    // Enumerate top-level windows and match the title substring instead.
    public static IntPtr Find(string like) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            if (IsWindowVisible(h)) {
                var sb = new StringBuilder(512); GetWindowTextW(h, sb, 512);
                if (sb.ToString().Contains(like)) { found = h; return false; }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

$hwnd = [Win32Cap]::Find($TitleMatch)
if ($hwnd -eq [IntPtr]::Zero) { Write-Error "No window matching '$TitleMatch'"; exit 1 }

[Win32Cap]::SetForegroundWindow($hwnd) | Out-Null
# Park the cursor over the viewport so hover highlights don't pollute the capture.
if (-not $KeepCursor) {
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point(10, 400)
}
Start-Sleep -Milliseconds 600

$rect = New-Object Win32Cap+RECT
[Win32Cap]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
if ($w -le 0 -or $h -le 0) { Write-Error "Bad window rect ${w}x${h}"; exit 1 }

$bmp = New-Object System.Drawing.Bitmap($w, $h)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bmp.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$gfx.Dispose(); $bmp.Dispose()
Write-Output "Saved ${w}x${h} -> $Out"
