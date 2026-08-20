# Placeholder Authenticode via cloud HSM (SSL.com eSigner or DigiCert KeyLocker).
# This script does not sign. It refuses so an unsigned Windows build is never
# treated as signed. Replace the body at go-live with CodeSignTool or smctl;
# do not store a raw Authenticode .pfx.
param(
    [Parameter(Mandatory = $true)]
    [string]$File
)

$ErrorActionPreference = "Stop"

function Test-Secret([string]$Name) {
    -not [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($Name))
}

$esigner = (Test-Secret "ES_USERNAME") -and (Test-Secret "ES_PASSWORD") -and (Test-Secret "ES_TOTP_SECRET") -and (Test-Secret "ES_CREDENTIAL_ID")
$keylocker = (Test-Secret "SM_HOST") -and (Test-Secret "SM_API_KEY") -and (Test-Secret "SM_CLIENT_CERT_FILE_B64") -and (Test-Secret "SM_CLIENT_CERT_PASSWORD") -and (Test-Secret "SM_KEYPAIR_ALIAS")

if (-not $esigner -and -not $keylocker) {
    Write-Error "Missing a complete Windows cloud-HSM secret set. Refusing unsigned Authenticode for $File"
    exit 1
}

Write-Error @"
Cloud HSM Authenticode is not live in this skeleton (no CodeSignTool / smctl invocation).
Refusing to publish an unsigned Windows build as if it were signed.
File: $File
When going live, invoke SSL.com eSigner CodeSignTool or DigiCert KeyLocker smctl here.
Do not store a raw Authenticode .pfx. Not Azure Trusted Signing.
"@
exit 1
