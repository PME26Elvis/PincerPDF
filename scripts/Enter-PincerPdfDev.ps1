param(
    [string]$DevRoot = $(if ($env:PINCERPDF_DEV_ROOT) { $env:PINCERPDF_DEV_ROOT } else { "D:\PincerPDF-dev" })
)

$ErrorActionPreference = "Stop"
$resolvedDevRoot = [System.IO.Path]::GetFullPath($DevRoot)

$env:PINCERPDF_DEV_ROOT = $resolvedDevRoot
$env:CARGO_HOME = Join-Path $resolvedDevRoot "rust\cargo"
$env:RUSTUP_HOME = Join-Path $resolvedDevRoot "rust\rustup"
$env:CARGO_TARGET_DIR = Join-Path $resolvedDevRoot "target"
$env:NPM_CONFIG_CACHE = Join-Path $resolvedDevRoot "cache\npm"
$env:PNPM_HOME = Join-Path $resolvedDevRoot "pnpm"
$env:PNPM_STORE_DIR = Join-Path $resolvedDevRoot "cache\pnpm-store"
$env:PNPM_CONFIG_STORE_DIR = $env:PNPM_STORE_DIR
$env:PNPM_CONFIG_VIRTUAL_STORE_DIR = Join-Path $resolvedDevRoot "frontend\node_modules\.pnpm"
$pythonSitePackages = Join-Path $resolvedDevRoot "python\site-packages"
$env:PYTHONPATH = @(
    $pythonSitePackages,
    (Join-Path $pythonSitePackages "win32"),
    (Join-Path $pythonSitePackages "win32\lib"),
    (Join-Path $pythonSitePackages "pythonwin")
) -join ";"
$env:PLAYWRIGHT_BROWSERS_PATH = Join-Path $resolvedDevRoot "cache\playwright"
$env:XDG_CACHE_HOME = Join-Path $resolvedDevRoot "cache"
$env:XDG_CONFIG_HOME = Join-Path $resolvedDevRoot "config"
$env:XDG_DATA_HOME = Join-Path $resolvedDevRoot "data"
$env:TEMP = Join-Path $resolvedDevRoot "tmp"
$env:TMP = $env:TEMP
if ($env:NO_COLOR -and $env:NO_COLOR -notin @("true", "false")) {
    $env:NO_COLOR = "true"
}

$vsDevCmd = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
if (Test-Path -LiteralPath $vsDevCmd) {
    $vsEnvironment = & cmd.exe /s /c "`"$vsDevCmd`" -arch=x64 -host_arch=x64 >nul && set"
    foreach ($line in $vsEnvironment) {
        if ($line -match "^([^=]+)=(.*)$") {
            [System.Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
        }
    }
}

$toolPaths = @(
    (Join-Path $env:CARGO_HOME "bin"),
    (Join-Path $resolvedDevRoot "tools\qpdf-11.3.0\qpdf-11.3.0-msvc64\bin"),
    (Join-Path $resolvedDevRoot "tools\mupdf-1.21.0\mupdf-1.21.0-windows"),
    (Join-Path $env:USERPROFILE ".cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin"),
    (Join-Path $env:USERPROFILE ".cache\codex-runtimes\codex-primary-runtime\dependencies\bin\fallback"),
    (Join-Path $env:USERPROFILE ".cache\codex-runtimes\codex-primary-runtime\dependencies\python")
)

foreach ($toolPath in $toolPaths) {
    if ((Test-Path -LiteralPath $toolPath) -and -not (($env:Path -split ";") -contains $toolPath)) {
        $env:Path = "$toolPath;$env:Path"
    }
}

$browserCandidates = @(
    "C:\Program Files\Google\Chrome\Application\chrome.exe",
    "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
)
$installedBrowser = $browserCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if ($installedBrowser) {
    $env:PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH = $installedBrowser
}

Write-Output "PincerPDF development environment: $resolvedDevRoot"
