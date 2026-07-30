[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot "Enter-PincerPdfDev.ps1")

if (-not $env:PINCERPDF_NATIVE_E2E_ARTIFACT_DIR) {
    $env:PINCERPDF_NATIVE_E2E_ARTIFACT_DIR = Join-Path $env:PINCERPDF_DEV_ROOT "artifacts\native-webview"
}

Push-Location (Join-Path $repositoryRoot "apps\pincerpdf-ui")
try {
    & trunk build --release
    if ($LASTEXITCODE -ne 0) {
        throw "Trunk release build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

Push-Location $repositoryRoot
try {
    & cargo build --locked --release -p pincerpdf-desktop --features tauri/custom-protocol
    if ($LASTEXITCODE -ne 0) {
        throw "Native Tauri release build failed with exit code $LASTEXITCODE."
    }

    & pnpm exec wdio run wdio.native.conf.mjs
    if ($LASTEXITCODE -ne 0) {
        throw "Native WebView E2E failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}
