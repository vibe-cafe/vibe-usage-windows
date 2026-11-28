# Prints the text of every visible text element in one window (the panel,
# by its handle), one per line, through UI Automation (what Narrator reads). Runs under Windows
# PowerShell 5.1, which always has the UI Automation client. Runner only.
param([Parameter(Mandatory = $true)][long]$WindowHandle)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$window = $A::FromHandle([IntPtr]$WindowHandle)
$text = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)
foreach ($element in $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $text)) {
    $name = $element.Current.Name
    if ($name -and -not $element.Current.IsOffscreen) { $name }
}
