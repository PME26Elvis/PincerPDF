param(
    [string]$DevRoot = $(if ($env:PINCERPDF_DEV_ROOT) { $env:PINCERPDF_DEV_ROOT } else { "D:\PincerPDF-dev" })
)

$ErrorActionPreference = "Stop"
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repositoryRoot = Split-Path -Parent $scriptRoot

. (Join-Path $scriptRoot "Enter-PincerPdfDev.ps1") -DevRoot $DevRoot
Set-Location -LiteralPath $repositoryRoot

& cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo test --locked --workspace --all-targets
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& python -m unittest discover -s scripts/tests
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& python scripts/verify-repo.py
exit $LASTEXITCODE
