param(
    [ValidateSet(
        "browser_indexeddb_transaction_matches_detached_control",
        "wasm_gba_browser_indexeddb_transaction_matches_detached_control",
        "wasm_sms_browser_indexeddb_transaction_matches_detached_control",
        "wasm_sms_browser_app_consumes_and_presents_detached_frame",
        "wasm_gba_browser_app_consumes_and_presents_detached_frame",
        "browser_netplay_audio_",
        "browser_netplay_certificate_",
        "wasm_coleco_"
    )]
    [string]$TestFilter = "browser_indexeddb_transaction_matches_detached_control",
    [switch]$Netplay,
    [switch]$NetplaySmoke,
    [switch]$MixedTransport,
    [switch]$MixedGameplay
)

$ErrorActionPreference = "Stop"
$runNetplay = $Netplay -or $NetplaySmoke
$runTransport = $runNetplay -or $MixedTransport
$runFixture = $runTransport -or $MixedGameplay
if (($Netplay -and $NetplaySmoke) -or ($MixedTransport -and $runNetplay) -or ($MixedGameplay -and $runTransport)) {
    throw "Select only one transport or netplay suite"
}

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
$netplayLobby = $null

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
                # Use Dawn's CPU adapter when hosted runners lack a GPU.
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
    $previousAddress = $env:WASM_BINDGEN_TEST_ADDRESS
    $previousFixtureRunId = $env:ZEFF_MIXED_TEST_RUN_ID
    try {
        $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $runner
        $env:RUST_LOG = "warn"
        $env:MSEDGEDRIVER = $driver
        $env:WASM_BINDGEN_TEST_WEBDRIVER_JSON = $webdriverConfig
        $env:WASM_BINDGEN_TEST_TIMEOUT = if ($runFixture) { "180" } else { "60" }
        $env:Path = "$(Split-Path -Parent $driver);$previousPath"
        if ($runFixture) {
            $fixtureRunId = Split-Path -Leaf $runRoot
            if ($MixedGameplay) {
                $artifactMessages = & cargo test --locked -p zeff-boy --bin zeff-boy --no-run --message-format=json
                if ($LASTEXITCODE -ne 0) {
                    $artifactMessages | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq "compiler-message" } | ForEach-Object { Write-Host $_.message.rendered }
                    throw "Native gameplay fixture build failed"
                }
                $fixtureArtifacts = @($artifactMessages | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object {
                    $_.reason -eq "compiler-artifact" -and $_.target.name -eq "zeff-boy" -and $_.profile.test -and $_.executable
                })
                if ($fixtureArtifacts.Count -ne 1) { throw "Expected one native gameplay test artifact" }
                $env:ZEFF_MIXED_TEST_RUN_ID = $fixtureRunId
                $netplayLobby = Start-Process -FilePath $fixtureArtifacts[0].executable -ArgumentList @(
                    "netplay::mixed_fixture::native::serve_mixed_gameplay_fixture", "--exact", "--ignored", "--nocapture", "--test-threads=1"
                ) -WindowStyle Hidden -PassThru
            } else {
                & cargo build --locked -p zeff-netplay-connect --features native --example browser-test-lobby
                if ($LASTEXITCODE -ne 0) { throw "Netplay lobby fixture build failed" }
                $netplayLobby = Start-Process -FilePath (Join-Path $targetRoot "debug\examples\browser-test-lobby.exe") -ArgumentList @("127.0.0.1:47180", "http://127.0.0.1:47181", $fixtureRunId) -WindowStyle Hidden -PassThru
            }
            $readyDeadline = [DateTime]::UtcNow.AddSeconds(10)
            while ($true) {
                if ($netplayLobby.HasExited) { throw "Netplay lobby fixture exited before readiness" }
                try {
                    $health = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:47180/test/mixed/health" -TimeoutSec 1
                    if ($health.StatusCode -eq 200 -and $health.Content.Trim() -eq $fixtureRunId) { break }
                } catch { }
                if ([DateTime]::UtcNow -ge $readyDeadline) { throw "Netplay lobby fixture readiness timed out" }
                Start-Sleep -Milliseconds 50
            }
            $env:WASM_BINDGEN_TEST_ADDRESS = "127.0.0.1:47181"
            if ($runTransport) {
                $transportFilter = if ($MixedTransport) { "browser_mixed_" } else { "browser_" }
                & cargo test --locked -p zeff-netplay-connect --target wasm32-unknown-unknown --features browser-tests $transportFilter
                if ($LASTEXITCODE -ne 0) { throw "Browser transport tests failed" }
            }
        }
        $effectiveTestFilter = if ($runNetplay) { "browser_netplay_" } else { $TestFilter }
        if ($MixedTransport) {
            return
        }
        if ($MixedGameplay) {
            & cargo test --locked --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features --features wasm-browser-tests browser_mixed_gameplay_
            if ($LASTEXITCODE -ne 0) { throw "Mixed gameplay tests failed" }
            return
        }
        if ($NetplaySmoke) {
            foreach ($smokeFilter in @(
                "browser_netplay_audio_",
                "browser_netplay_actual_worker_matches_reference_and_restores_sram",
                "browser_netplay_ws_worker_replicates_link_and_restores",
                "browser_netplay_build_mismatch_and_suspended_peer_restore_before_saving"
            )) {
                & cargo test --locked --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features --features wasm-browser-tests $smokeFilter
                if ($LASTEXITCODE -ne 0) { throw "Browser netplay smoke failed: $smokeFilter" }
            }
        } elseif ($Netplay) {
            & cargo test --locked --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features --features wasm-browser-tests $effectiveTestFilter -- --skip browser_netplay_pce_ --skip browser_netplay_ws_ --skip browser_netplay_wsc_
            if ($LASTEXITCODE -ne 0) { throw "Browser netplay tests failed" }
            foreach ($coreFilter in @("browser_netplay_pce_base_", "browser_netplay_pce_supergrafx_", "browser_netplay_ws_", "browser_netplay_wsc_")) {
                & cargo test --locked --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features --features wasm-browser-tests $coreFilter
                if ($LASTEXITCODE -ne 0) { throw "Browser core tests failed: $coreFilter" }
            }
            & cargo test --locked --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features --features wasm-browser-tests native_wasm_nes_portability_receipts
            if ($LASTEXITCODE -ne 0) { throw "Native/browser portability tests failed" }
        } else {
            & cargo test --locked --package zeff-boy --bin zeff-boy --target wasm32-unknown-unknown --no-default-features --features wasm-browser-tests $effectiveTestFilter
            if ($LASTEXITCODE -ne 0) { throw "Browser WASM speculation tests failed" }
        }
    } finally {
        $env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $previousRunner
        $env:RUST_LOG = $previousRustLog
        $env:MSEDGEDRIVER = $previousDriver
        $env:WASM_BINDGEN_TEST_WEBDRIVER_JSON = $previousWebDriverConfig
        $env:WASM_BINDGEN_TEST_TIMEOUT = $previousTestTimeout
        $env:Path = $previousPath
        $env:WASM_BINDGEN_TEST_ADDRESS = $previousAddress
        $env:ZEFF_MIXED_TEST_RUN_ID = $previousFixtureRunId
        if ($netplayLobby -and -not $netplayLobby.HasExited) {
            Stop-Process -Id $netplayLobby.Id
            $netplayLobby.WaitForExit()
        }
    }
} finally {
    try {
        Remove-TestRun $runRoot
    } finally {
        Pop-Location
    }
}
