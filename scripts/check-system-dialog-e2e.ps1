[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot "Enter-PincerPdfDev.ps1")

$fixtureRoot = Join-Path $env:PINCERPDF_DEV_ROOT "artifacts\pdf-engine-probe\fixtures"
$outputRoot = Join-Path $env:PINCERPDF_DEV_ROOT "artifacts\system-dialog"
$pythonSitePackages = Join-Path $env:PINCERPDF_DEV_ROOT "python\site-packages"
New-Item -ItemType Directory -Force -Path $fixtureRoot, $outputRoot | Out-Null

if (-not (Test-Path -LiteralPath (Join-Path $pythonSitePackages "pywinauto"))) {
    New-Item -ItemType Directory -Force -Path $pythonSitePackages | Out-Null
    & python -m pip install `
        --disable-pip-version-check `
        --target $pythonSitePackages `
        --requirement (Join-Path $repositoryRoot "requirements-windows-e2e.txt")
    if ($LASTEXITCODE -ne 0) {
        throw "Windows UI automation dependency installation failed with exit code $LASTEXITCODE."
    }
}

& python (Join-Path $repositoryRoot "tests\fixtures\pdf\generate_fixtures.py") $fixtureRoot
if ($LASTEXITCODE -ne 0) {
    throw "PDF fixture generation failed with exit code $LASTEXITCODE."
}

$env:PINCERPDF_SYSTEM_DIALOG_E2E = "1"
$env:PINCERPDF_SYSTEM_DIALOG_FIXTURES = $fixtureRoot
$env:PINCERPDF_SYSTEM_DIALOG_OUTPUT_DIR = $outputRoot
$env:PINCERPDF_NATIVE_E2E_ARTIFACT_DIR = Join-Path $env:PINCERPDF_DEV_ROOT "artifacts\native-webview"

Push-Location $repositoryRoot
try {
    if (-not $SkipBuild) {
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

        & cargo build --locked --release -p pincerpdf-desktop --features tauri/custom-protocol
        if ($LASTEXITCODE -ne 0) {
            throw "Native Tauri release build failed with exit code $LASTEXITCODE."
        }
    }

    & pnpm exec wdio run wdio.native.conf.mjs `
        --spec tests/native/merge-system-dialog.e2e.mjs
    if ($LASTEXITCODE -ne 0) {
        throw "Windows system-dialog E2E failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}
