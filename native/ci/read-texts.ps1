# Prints the text of every visible text element in a process's windows, one
# per line, through UI Automation (what Narrator reads). Runs under Windows
# PowerShell 5.1, which always has the UI Automation client. Runner only.
param([Parameter(Mandatory = $true)][int]$ProcessId)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$windows = $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,
    (New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $ProcessId)))
$text = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)
foreach ($window in $windows) {
    foreach ($element in $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $text)) {
        $name = $element.Current.Name
        if ($name -and -not $element.Current.IsOffscreen) { $name }
    }
}
