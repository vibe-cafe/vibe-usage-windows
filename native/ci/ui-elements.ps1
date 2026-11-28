# Writes the panel's UI Automation elements (what Narrator reads) to a UTF-8
# file, one per line: control type, name, screen rectangle (physical px),
# enabled, keyboard-focusable. Runs under Windows PowerShell 5.1, which always
# has the UI Automation client. Runner only.
param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][string]$OutFile
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class VuDpiAware2 {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
}
"@
[VuDpiAware2]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$scope = [System.Windows.Automation.TreeScope]
$cond = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $ProcessId)),
    (New-Object System.Windows.Automation.PropertyCondition($A::NameProperty, 'Vibe Usage')))
$lines = New-Object System.Collections.Generic.List[string]
foreach ($w in $A::RootElement.FindAll($scope::Children, $cond)) {
    foreach ($e in $w.FindAll($scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
        $c = $e.Current
        if (-not $c.Name) { continue }
        $r = $c.BoundingRectangle
        if ($r.IsEmpty) { continue }
        $lines.Add(("{0}`t{1}`t{2}`t{3}`t{4}`t{5}`t{6}`t{7}" -f $c.ControlType.ProgrammaticName.Replace('ControlType.', ''), $c.Name,
            [int]$r.X, [int]$r.Y, [int]$r.Width, [int]$r.Height, $c.IsEnabled, $c.IsKeyboardFocusable))
    }
}
[System.IO.File]::WriteAllLines($OutFile, $lines, (New-Object System.Text.UTF8Encoding($false)))
