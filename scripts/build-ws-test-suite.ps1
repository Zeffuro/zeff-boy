param(
    [ValidateSet("auto", "docker", "local")]
    [string]$Builder = "auto",
    [string]$SourceCacheDir = "rom-tests/cache/_sources",
    [string]$BuildDir = "rom-tests/cache/_build/asiekierka-ws-test-suite-7dfa0e2e",
    [string]$OutDir = "rom-tests/cache/ws/asiekierka/ws-test-suite",
    [string]$DockerImage = "cbrzeszczot/wonderful:wswan-latest",
    [switch]$UpdateToolchain,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$sourceId = "asiekierka-ws-test-suite-7dfa0e2e"
$commit = "7dfa0e2e869d08386b685d6a56df0bcfaf181b47"
$archiveName = "$commit.zip"
$url = "https://github.com/asiekierka/ws-test-suite/archive/$commit.zip"
$expectedSha256 = "5e9e3f10acc45fcdf6567cc7e2fe540889589e87e27e6025e66d81bf3ee58d88"
$archivePrefix = "ws-test-suite-$commit"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cacheRoot = Join-Path $repoRoot "rom-tests/cache"
$sourceCachePath = Join-Path $repoRoot $SourceCacheDir
$archiveDir = Join-Path $sourceCachePath $sourceId
$archivePath = Join-Path $archiveDir $archiveName
$buildPath = Join-Path $repoRoot $BuildDir
$outFullPath = Join-Path $repoRoot $OutDir

function Get-FullPath([string]$Path) {
    return [System.IO.Path]::GetFullPath($Path)
}

function Assert-UnderCache([string]$Path, [string]$Description) {
    $resolvedCacheRoot = Get-FullPath $cacheRoot
    $resolvedPath = Get-FullPath $Path
    $cachePrefix = $resolvedCacheRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedPath.StartsWith($cachePrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to operate on $Description outside rom-tests/cache: $resolvedPath"
    }
}

function Remove-CacheDirectoryIfPresent([string]$Path, [string]$Description) {
    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }

    Assert-UnderCache $Path $Description
    $resolvedPath = (Resolve-Path -LiteralPath $Path).Path
    if ($DryRun) {
        Write-Host "Would remove $Description at $resolvedPath"
        return
    }

    Remove-Item -LiteralPath $resolvedPath -Recurse -Force
}

function Invoke-Checked([string]$FilePath, [string[]]$Arguments, [string]$FailureMessage) {
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw $FailureMessage
    }
}

function Test-DockerAvailable {
    return [bool](Get-Command docker -ErrorAction SilentlyContinue)
}

function Test-LocalWonderfulAvailable {
    if (-not (Get-Command make -ErrorAction SilentlyContinue)) {
        return $false
    }

    if ($env:WONDERFUL_TOOLCHAIN) {
        return $true
    }

    return Test-Path -LiteralPath "/opt/wonderful"
}

function Resolve-Builder {
    if ($Builder -ne "auto") {
        return $Builder
    }

    if (Test-LocalWonderfulAvailable) {
        return "local"
    }

    if (Test-DockerAvailable) {
        return "docker"
    }

    throw "No supported ws-test-suite builder found. Install Docker Desktop or run from a Wonderful Toolchain shell with make available."
}

function Invoke-DockerBuild([string]$SourcePath) {
    if (-not (Test-DockerAvailable)) {
        throw "Docker is required for Builder=docker. Install/start Docker Desktop or use -Builder local from a Wonderful Toolchain shell."
    }

    if ($DryRun) {
        Write-Host "Would build with Docker image $DockerImage"
        return
    }

    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    docker image inspect $DockerImage *> $null
    $imageInspectExitCode = $LASTEXITCODE
    $ErrorActionPreference = $oldErrorActionPreference

    if ($imageInspectExitCode -ne 0) {
        Write-Host "Pulling $DockerImage..."
        Invoke-Checked "docker" @("pull", $DockerImage) "Failed to pull $DockerImage. Is Docker Desktop running?"
    }

    $buildCommand = "make TARGET=wswan/small"
    if ($UpdateToolchain) {
        $buildCommand = "wf-pacman -Syu --noconfirm && wf-pacman -Su --noconfirm && $buildCommand"
    }

    Invoke-Checked "docker" @(
        "run",
        "--rm",
        "-v",
        "${SourcePath}:/workspace",
        "-w",
        "/workspace",
        $DockerImage,
        "sh",
        "-lc",
        $buildCommand
    ) "ws-test-suite Docker build failed"
}

function Invoke-LocalBuild([string]$SourcePath) {
    $make = Get-Command make -ErrorAction SilentlyContinue
    if (-not $make) {
        throw "make is required for Builder=local. Run from a Wonderful Toolchain shell or use -Builder docker."
    }

    if (-not $env:WONDERFUL_TOOLCHAIN -and -not (Test-Path -LiteralPath "/opt/wonderful")) {
        throw "Wonderful Toolchain was not found. Set WONDERFUL_TOOLCHAIN or install it at /opt/wonderful."
    }

    if ($DryRun) {
        Write-Host "Would build locally with $($make.Source)"
        return
    }

    Push-Location $SourcePath
    try {
        Invoke-Checked $make.Source @("TARGET=wswan/small") "ws-test-suite local build failed"
    } finally {
        Pop-Location
    }
}

New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null

Write-Host "Source: $sourceId @ $commit"
Write-Host "Archive: $archivePath"
Write-Host "Output: $OutDir"

if ($DryRun) {
    Write-Host "Dry run enabled; no files will be downloaded, removed, built, or copied."
} else {
    New-Item -ItemType Directory -Force -Path $archiveDir | Out-Null
    if (-not (Test-Path -LiteralPath $archivePath)) {
        Invoke-WebRequest -Uri $url -OutFile $archivePath -Headers @{ "User-Agent" = "zeff-romtest" }
    }

    $actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
    if ($actualSha256 -ne $expectedSha256) {
        throw "ws-test-suite source hash mismatch: expected $expectedSha256, got $actualSha256"
    }
}

$resolvedBuilder = Resolve-Builder
Write-Host "Builder: $resolvedBuilder"

Remove-CacheDirectoryIfPresent $buildPath "ws-test-suite build directory"
Remove-CacheDirectoryIfPresent $outFullPath "ws-test-suite output directory"

if ($DryRun) {
    Write-Host "Would expand $archivePath to $buildPath"
    Write-Host "Would copy build/roms/*.ws and build/roms/*.wsc to $OutDir"
    exit 0
}

New-Item -ItemType Directory -Force -Path $buildPath | Out-Null
Expand-Archive -LiteralPath $archivePath -DestinationPath $buildPath -Force
$sourcePath = Join-Path $buildPath $archivePrefix

if (-not (Test-Path -LiteralPath (Join-Path $sourcePath "Makefile"))) {
    throw "Expanded source did not contain expected Makefile at $sourcePath"
}

switch ($resolvedBuilder) {
    "docker" { Invoke-DockerBuild $sourcePath }
    "local" { Invoke-LocalBuild $sourcePath }
    default { throw "Unsupported builder: $resolvedBuilder" }
}

$builtRomsDir = Join-Path $sourcePath "build/roms"
if (-not (Test-Path -LiteralPath $builtRomsDir)) {
    throw "ws-test-suite build completed but build/roms was not produced"
}

$builtRomsRoot = (Resolve-Path -LiteralPath $builtRomsDir).Path.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
$builtRoms = @(Get-ChildItem -LiteralPath $builtRomsDir -Recurse -File |
    Where-Object { $_.Extension -in @(".ws", ".wsc") } |
    Sort-Object FullName)

if ($builtRoms.Count -eq 0) {
    throw "ws-test-suite build completed but no .ws/.wsc ROMs were produced"
}

New-Item -ItemType Directory -Force -Path $outFullPath | Out-Null
foreach ($rom in $builtRoms) {
    $relativePath = $rom.FullName.Substring($builtRomsRoot.Length).TrimStart([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    $destination = Join-Path $outFullPath $relativePath
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
    Copy-Item -LiteralPath $rom.FullName -Destination $destination -Force
}

$licensePath = Join-Path $sourcePath "LICENSE"
if (Test-Path -LiteralPath $licensePath) {
    Copy-Item -LiteralPath $licensePath -Destination (Join-Path $outFullPath "LICENSE.ws-test-suite.txt") -Force
}

Write-Host "Wrote $($builtRoms.Count) WS test suite ROM(s) to $OutDir"
