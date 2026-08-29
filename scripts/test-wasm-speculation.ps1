$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$toolRoot = Join-Path $repoRoot "target\wasm-bindgen-cli-0.2.126"
$runner = Join-Path $toolRoot "bin\wasm-bindgen-test-runner.exe"

Push-Location $repoRoot
try {
    if (-not (Test-Path -LiteralPath $runner)) {
        & cargo install wasm-bindgen-cli --version 0.2.126 --locked --root $toolRoot
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    }

    $previousRunner = $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER
    $previousRustLog = $env:RUST_LOG
    try {
        $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $runner
        $env:RUST_LOG = "warn"
        & cargo test --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features wasm_sms_detached
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    } finally {
        $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $previousRunner
        $env:RUST_LOG = $previousRustLog
    }
} finally {
    Pop-Location
}
