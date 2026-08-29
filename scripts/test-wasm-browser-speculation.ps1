param(
    [ValidateSet(
        "browser_indexeddb_transaction_matches_detached_control",
        "wasm_gba_browser_indexeddb_transaction_matches_detached_control",
        "wasm_sms_browser_indexeddb_transaction_matches_detached_control",
        "wasm_sms_browser_app_consumes_and_presents_detached_frame",
        "wasm_gba_browser_app_consumes_and_presents_detached_frame"
    )]
    [string]$TestFilter = "browser_indexeddb_transaction_matches_detached_control"
)

$ErrorActionPreference = "Stop"

$repoRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$targetRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot "target"))
$toolRoot = Join-Path $targetRoot "wasm-browser-tools"
$runnerRoot = Join-Path $toolRoot "wasm-bindgen-cli-0.2.126"
$runner = Join-Path $runnerRoot "bin\wasm-bindgen-test-runner.exe"
$edgePath = "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

function Read-BinaryVersion([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label was not found at $Path"
    }
    $reported = (Get-Item -LiteralPath $Path).VersionInfo.ProductVersion
    $match = [regex]::Match($reported, '\d+\.\d+\.\d+\.\d+')
    if (-not $match.Success) {
        throw "$Label reported an invalid version: $reported"
    }
    $match.Value
}

function Version-Triplet([string]$Version) {
    ($Version.Split('.')[0..2] -join '.')
}

function Remove-TestRun([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }
    $resolved = [IO.Path]::GetFullPath($Path)
    $allowedRoot = [IO.Path]::GetFullPath((Join-Path $targetRoot "wasm-browser-runs"))
    $allowedPrefix = $allowedRoot.TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($allowedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove browser test path outside $allowedRoot"
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}

$edgeVersion = Read-BinaryVersion $edgePath "Microsoft Edge"
$driverRoot = Join-Path $toolRoot "msedgedriver-$edgeVersion"
$driver = Join-Path $driverRoot "msedgedriver.exe"
$archive = Join-Path $toolRoot "downloads\edgedriver-$edgeVersion-win64.zip"
$runRoot = Join-Path $targetRoot "wasm-browser-runs\$([guid]::NewGuid().ToString('N'))"
$profile = Join-Path $runRoot "edge-profile"
$webdriverConfig = Join-Path $runRoot "webdriver.json"

Push-Location $repoRoot
try {
    if (-not (Test-Path -LiteralPath $runner -PathType Leaf)) {
        & cargo install wasm-bindgen-cli --version 0.2.126 --locked --root $runnerRoot
        if ($LASTEXITCODE -ne 0) {
            throw "wasm-bindgen-cli installation failed with exit code $LASTEXITCODE"
        }
    }

    if (-not (Test-Path -LiteralPath $driver -PathType Leaf)) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $archive) | Out-Null
        $driverUrl = "https://msedgedriver.microsoft.com/$edgeVersion/edgedriver_win64.zip"
        Invoke-WebRequest -UseBasicParsing -Uri $driverUrl -OutFile $archive
        New-Item -ItemType Directory -Force -Path $driverRoot | Out-Null
        Expand-Archive -LiteralPath $archive -DestinationPath $driverRoot -Force
    }
    if (-not (Test-Path -LiteralPath $driver -PathType Leaf)) {
        throw "EdgeDriver archive did not contain msedgedriver.exe"
    }

    $driverOutput = (& $driver --version | Out-String).Trim()
    $driverMatch = [regex]::Match($driverOutput, '\d+\.\d+\.\d+\.\d+')
    if (-not $driverMatch.Success) {
        throw "EdgeDriver reported an invalid version: $driverOutput"
    }
    $driverVersion = $driverMatch.Value
    if ((Version-Triplet $edgeVersion) -ne (Version-Triplet $driverVersion)) {
        throw "Edge $edgeVersion and EdgeDriver $driverVersion do not match through build version"
    }

    New-Item -ItemType Directory -Force -Path $profile | Out-Null
    $capabilities = @{
        "ms:edgeOptions" = @{
            binary = $edgePath
            args = @(
                "user-data-dir=$profile"
                "no-first-run"
                "no-default-browser-check"
                # Hosted runners do not expose a physical GPU. Keep this a real
                # WebGPU surface/present proof by selecting Chromium's CPU Dawn
                # adapter instead of falling back to WebGL or skipping graphics.
                "enable-unsafe-webgpu"
                "use-webgpu-adapter=swiftshader"
                "use-gpu-in-tests"
            )
        }
    } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText(
        $webdriverConfig,
        $capabilities,
        [Text.UTF8Encoding]::new($false)
    )

    $previousRunner = $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER
    $previousRustLog = $env:RUST_LOG
    $previousDriver = $env:MSEDGEDRIVER
    $previousWebDriverConfig = $env:WASM_BINDGEN_TEST_WEBDRIVER_JSON
    $previousTestTimeout = $env:WASM_BINDGEN_TEST_TIMEOUT
    $previousPath = $env:Path
    try {
        $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $runner
        $env:RUST_LOG = "warn"
        $env:MSEDGEDRIVER = $driver
        $env:WASM_BINDGEN_TEST_WEBDRIVER_JSON = $webdriverConfig
        $env:WASM_BINDGEN_TEST_TIMEOUT = "60"
        $env:Path = "$(Split-Path -Parent $driver);$previousPath"
        & cargo test --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features --features wasm-browser-tests $TestFilter
        if ($LASTEXITCODE -ne 0) {
            throw "browser WASM speculation test failed with exit code $LASTEXITCODE"
        }
    } finally {
        $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $previousRunner
        $env:RUST_LOG = $previousRustLog
        $env:MSEDGEDRIVER = $previousDriver
        $env:WASM_BINDGEN_TEST_WEBDRIVER_JSON = $previousWebDriverConfig
        $env:WASM_BINDGEN_TEST_TIMEOUT = $previousTestTimeout
        $env:Path = $previousPath
    }
} finally {
    try {
        Remove-TestRun $runRoot
    } finally {
        Pop-Location
    }
}
