$ErrorActionPreference = "Stop"

$bundles = if ($env:TAURI_BUNDLES) { $env:TAURI_BUNDLES } else { "nsis" }
$buildArgs = @("tauri", "build", "--bundles", $bundles)
$certThumbprint = $env:WINDOWS_CODESIGN_CERT_THUMBPRINT
$useSignPath = [bool]$env:SIGNPATH_API_TOKEN
$allowUntrustedSignature = $env:SIGNPATH_ALLOW_UNTRUSTED_SIGNATURE -match '^(1|true|yes|on)$'
$signingEnabled = $false
$tempFiles = @()

if (-not $useSignPath -and -not $certThumbprint -and $env:WINDOWS_CODESIGN_PFX_BASE64) {
  if (-not $env:WINDOWS_CODESIGN_PFX_PASSWORD) {
    throw "WINDOWS_CODESIGN_PFX_PASSWORD is required when WINDOWS_CODESIGN_PFX_BASE64 is set."
  }

  $pfxPath = Join-Path ([System.IO.Path]::GetTempPath()) "vibe-usage-codesign-$PID.pfx"
  [System.IO.File]::WriteAllBytes($pfxPath, [Convert]::FromBase64String($env:WINDOWS_CODESIGN_PFX_BASE64))
  $tempFiles += $pfxPath

  $password = ConvertTo-SecureString $env:WINDOWS_CODESIGN_PFX_PASSWORD -AsPlainText -Force
  $cert = Import-PfxCertificate -FilePath $pfxPath -CertStoreLocation Cert:\CurrentUser\My -Password $password
  if (-not $cert) {
    throw "Failed to import Windows code signing certificate."
  }
  $certThumbprint = $cert.Thumbprint
}

if ($useSignPath) {
  $signingConfigPath = Join-Path ([System.IO.Path]::GetTempPath()) "tauri-signpath-$PID.json"
  $signPathScript = Join-Path $PSScriptRoot "signpath-tauri-sign.ps1"
  $signingConfig = @{
    bundle = @{
      windows = @{
        digestAlgorithm = "sha256"
        signCommand = @{
          cmd = "pwsh"
          args = @(
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            $signPathScript,
            "%1"
          )
        }
      }
    }
  } | ConvertTo-Json -Depth 10
  Set-Content -LiteralPath $signingConfigPath -Value $signingConfig -Encoding UTF8
  $tempFiles += $signingConfigPath
  $buildArgs += @("--config", $signingConfigPath)
  $signingEnabled = $true
  Write-Host "== Windows code signing enabled: SignPath =="
} elseif ($certThumbprint) {
  $timestampUrl = if ($env:WINDOWS_CODESIGN_TIMESTAMP_URL) {
    $env:WINDOWS_CODESIGN_TIMESTAMP_URL
  } else {
    "http://timestamp.digicert.com"
  }
  $signingConfigPath = Join-Path ([System.IO.Path]::GetTempPath()) "tauri-signing-$PID.json"
  $signingConfig = @{
    bundle = @{
      windows = @{
        digestAlgorithm = "sha256"
        certificateThumbprint = $certThumbprint
        timestampUrl = $timestampUrl
      }
    }
  } | ConvertTo-Json -Depth 5
  Set-Content -LiteralPath $signingConfigPath -Value $signingConfig -Encoding UTF8
  $tempFiles += $signingConfigPath
  $buildArgs += @("--config", $signingConfigPath)
  $signingEnabled = $true
  Write-Host "== Windows code signing enabled =="
} else {
  Write-Host "== Windows code signing disabled: no certificate configured =="
}

try {
  & pnpm @buildArgs
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }

  if ($signingEnabled) {
    $version = (Get-Content package.json | ConvertFrom-Json).version
    $installer = Get-ChildItem -Path "target\release\bundle\nsis" -Filter "*$version*setup.exe" |
      Sort-Object LastWriteTime -Descending |
      Select-Object -First 1
    if (-not $installer) {
      throw "NSIS installer not found for version $version."
    }

    $artifactsToVerify = if ($useSignPath) {
      @($installer.FullName)
    } else {
      @("target\release\vibe-usage-app.exe", $installer.FullName)
    }

    foreach ($artifact in $artifactsToVerify) {
      $signature = Get-AuthenticodeSignature $artifact
      $signatureStatus = $signature.Status.ToString()
      if ($signatureStatus -eq "NotSigned" -or $signatureStatus -eq "HashMismatch") {
        throw "Invalid Authenticode signature for ${artifact}: $($signature.Status)"
      }
      if ($signatureStatus -ne "Valid") {
        $message = "Untrusted Authenticode signature for ${artifact}: $($signature.Status). $($signature.StatusMessage)"
        if ($allowUntrustedSignature) {
          Write-Warning $message
        } else {
          throw $message
        }
      }
    }
    Write-Host "== Windows code signatures verified =="
  }
} finally {
  foreach ($tempFile in $tempFiles) {
    Remove-Item -LiteralPath $tempFile -Force -ErrorAction SilentlyContinue
  }
}
