# Prints the screen centre (physical px) of the taskbar's 「^」 button that
# opens the notification overflow, or "none". Runs under Windows PowerShell
# 5.1, which always has the UI Automation client. Runner only.
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class VuDpiAware {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
}
"@
[VuDpiAware]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$root = $A::RootElement
$tray = $root.FindFirst([System.Windows.Automation.TreeScope]::Children,
    (New-Object System.Windows.Automation.PropertyCondition($A::ClassNameProperty, 'Shell_TrayWnd')))
if ($null -eq $tray) { 'none'; exit 0 }
$all = $tray.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
$hit = $null
foreach ($e in $all) {
    $c = $e.Current
    $r = $c.BoundingRectangle
    # Diagnostics for the log: what the notification area offers.
    if ($c.Name -or $c.AutomationId) {
        [Console]::Error.WriteLine("uia: '$($c.Name)' id='$($c.AutomationId)' class='$($c.ClassName)' at $([int]$r.X),$([int]$r.Y) $([int]$r.Width)x$([int]$r.Height)")
    }
    # Match by name only: every notification icon has AutomationId SystemTrayIcon.
    if ($null -eq $hit -and $c.Name -match '^(Show Hidden Icons|Show hidden icons|显示隐藏的图标)') { $hit = $c }
}
if ($null -eq $hit) { 'none'; exit 0 }
$r = $hit.BoundingRectangle
"{0} {1}" -f [int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)
