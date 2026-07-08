param(
  [Parameter(Mandatory = $true)]
  [string]$InputArtifactPath,

  [Parameter(Mandatory = $true)]
  [string]$OutputArtifactPath,

  [string]$OrganizationId = "ac47326c-b8ee-4b87-9368-b7af8a8803d7",
  [string]$ProjectSlug = "vibe_usage",
  [string]$SigningPolicySlug = "Release",
  [string]$ArtifactConfigurationSlug = "",
  [string]$Description = "",
  [int]$TimeoutSeconds = 900,
  [int]$PollSeconds = 10,
  [switch]$AllowUntrustedSignature
)

$ErrorActionPreference = "Stop"

if (-not $env:SIGNPATH_API_TOKEN) {
  throw "SIGNPATH_API_TOKEN is required."
}

if ($env:SIGNPATH_ORGANIZATION_ID) {
  $OrganizationId = $env:SIGNPATH_ORGANIZATION_ID
}
if ($env:SIGNPATH_PROJECT_SLUG) {
  $ProjectSlug = $env:SIGNPATH_PROJECT_SLUG
}
if ($env:SIGNPATH_SIGNING_POLICY_SLUG) {
  $SigningPolicySlug = $env:SIGNPATH_SIGNING_POLICY_SLUG
}
if ($env:SIGNPATH_ARTIFACT_CONFIGURATION_SLUG) {
  $ArtifactConfigurationSlug = $env:SIGNPATH_ARTIFACT_CONFIGURATION_SLUG
}

$inputArtifact = Get-Item -LiteralPath $InputArtifactPath
$outputFullPath = [System.IO.Path]::GetFullPath($OutputArtifactPath)
$outputDirectory = Split-Path -Parent $outputFullPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null

$headers = @{
  Authorization = "Bearer $env:SIGNPATH_API_TOKEN"
}

$submitUri = "https://app.signpath.io/api/v1/$OrganizationId/SigningRequests/SubmitWithArtifact"
$form = @{
  projectSlug = $ProjectSlug
  signingPolicySlug = $SigningPolicySlug
  artifact = $inputArtifact
}

if ($ArtifactConfigurationSlug) {
  $form.artifactConfigurationSlug = $ArtifactConfigurationSlug
}

if ($Description) {
  $form.description = $Description
}

Write-Host "== Submitting $($inputArtifact.Name) to SignPath policy '$SigningPolicySlug' =="
$submitResponse = Invoke-WebRequest -Method Post -Uri $submitUri -Headers $headers -Form $form
if ($submitResponse.StatusCode -ne 201) {
  throw "SignPath submit failed with HTTP $($submitResponse.StatusCode)."
}

$requestUri = $submitResponse.Headers.Location
if ($requestUri -is [array]) {
  $requestUri = $requestUri[0]
}
if (-not $requestUri) {
  throw "SignPath submit response did not include a Location header."
}

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
do {
  $status = Invoke-RestMethod -Method Get -Uri $requestUri -Headers $headers
  Write-Host "SignPath status: $($status.status) / $($status.workflowStatus)"

  if ($status.isFinalStatus) {
    break
  }

  if ((Get-Date) -ge $deadline) {
    throw "Timed out waiting for SignPath signing request $($status.signingRequestId)."
  }

  Start-Sleep -Seconds $PollSeconds
} while ($true)

if ($status.status -ne "Completed") {
  $statusJson = $status | ConvertTo-Json -Depth 20
  throw "SignPath signing request did not complete successfully: $statusJson"
}

if (-not $status.signedArtifactLink) {
  throw "SignPath completed request did not include signedArtifactLink."
}

Invoke-WebRequest -Method Get -Uri $status.signedArtifactLink -Headers $headers -OutFile $outputFullPath
$signature = Get-AuthenticodeSignature -FilePath $outputFullPath
if ($signature.SignerCertificate) {
  Write-Host "Signer certificate: $($signature.SignerCertificate.Subject)"
}
Write-Host "Authenticode status: $($signature.Status)"

$signatureStatus = $signature.Status.ToString()
if ($signatureStatus -eq "NotSigned") {
  throw "Downloaded SignPath artifact is not Authenticode signed."
}

if ($signatureStatus -eq "HashMismatch") {
  throw "Invalid Authenticode signature for ${outputFullPath}: $($signature.Status). $($signature.StatusMessage)"
}

if ($signatureStatus -ne "Valid") {
  $message = "Invalid Authenticode signature for ${outputFullPath}: $($signature.Status). $($signature.StatusMessage)"
  if ($AllowUntrustedSignature) {
    Write-Warning $message
  } else {
    throw $message
  }
}

Write-Host "== SignPath signed artifact saved to $outputFullPath =="
