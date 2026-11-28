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
    if ($c.AutomationId -eq 'SystemTrayIcon' -or $c.Name -match '^(Show Hidden Icons|Show hidden icons|显示隐藏的图标)') { $hit = $c; break }
}
if ($null -eq $hit) {
    # Diagnostics for the log: what the notification area offers.
    foreach ($e in $all) {
        $c = $e.Current
        if ($c.Name) { [Console]::Error.WriteLine("uia: '$($c.Name)' id='$($c.AutomationId)' class='$($c.ClassName)'") }
    }
    'none'
    exit 0
}
$r = $hit.BoundingRectangle
"{0} {1}" -f [int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)
