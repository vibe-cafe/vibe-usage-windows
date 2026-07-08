param(
  [Parameter(Mandatory = $true, Position = 0)]
  [string]$Path
)

$ErrorActionPreference = "Stop"

$artifact = Get-Item -LiteralPath $Path
$tempRoot = [System.IO.Path]::GetTempPath()
$tempDir = Join-Path $tempRoot "vibe-usage-signpath-$([System.Guid]::NewGuid().ToString('N'))"
$signedArtifactPath = Join-Path $tempDir $artifact.Name
$allowUntrustedSignature = $env:SIGNPATH_ALLOW_UNTRUSTED_SIGNATURE -match '^(1|true|yes|on)$'

New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

try {
  $descriptionPrefix = if ($env:SIGNPATH_DESCRIPTION_PREFIX) {
    $env:SIGNPATH_DESCRIPTION_PREFIX
  } else {
    "Vibe Usage"
  }

  $description = "$descriptionPrefix $($artifact.Name)"
  if ($env:GITHUB_SHA) {
    $description = "$description $env:GITHUB_SHA"
  }

  $signArgs = @{
    InputArtifactPath = $artifact.FullName
    OutputArtifactPath = $signedArtifactPath
    Description = $description
  }

  if ($allowUntrustedSignature) {
    $signArgs.AllowUntrustedSignature = $true
  }

  & (Join-Path $PSScriptRoot "signpath-sign-artifact.ps1") @signArgs

  Copy-Item -LiteralPath $signedArtifactPath -Destination $artifact.FullName -Force

  $signature = Get-AuthenticodeSignature -FilePath $artifact.FullName
  $signatureStatus = $signature.Status.ToString()
  if ($signatureStatus -eq "NotSigned" -or $signatureStatus -eq "HashMismatch") {
    throw "Invalid Authenticode signature after replacing ${Path}: $($signature.Status). $($signature.StatusMessage)"
  }

  if ($signatureStatus -ne "Valid") {
    $message = "Untrusted Authenticode signature after replacing ${Path}: $($signature.Status). $($signature.StatusMessage)"
    if ($allowUntrustedSignature) {
      Write-Warning $message
    } else {
      throw $message
    }
  }

  Write-Host "== SignPath replaced $($artifact.FullName) with a signed artifact =="
} finally {
  if (Test-Path -LiteralPath $tempDir) {
    $resolvedTempDir = (Resolve-Path -LiteralPath $tempDir).Path
    if ($resolvedTempDir.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
      Remove-Item -LiteralPath $resolvedTempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
}
