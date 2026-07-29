param(
    [string]$SourceCacheDir = "rom-tests/cache/_sources",
    [string]$BuildDir = "rom-tests/cache/_build/mgba-suite-e6942030",
    [string]$OutPath = "rom-tests/cache/gba/mgba-suite/suite.gba"
)

$ErrorActionPreference = "Stop"

$sourceId = "mgba-suite-e6942030"
$commit = "e6942030d25ffe3ba76c72b73a86da073ec857cc"
$archiveName = "$commit.zip"
$url = "https://github.com/mgba-emu/suite/archive/$commit.zip"
$expectedSha256 = "4a82b13a84c5a5c904bdb73c1dab4d3a3c78b45e9fa2000e10553ac1ce19a585"
$archivePrefix = "suite-$commit"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$sourceCachePath = Join-Path $repoRoot $SourceCacheDir
$archiveDir = Join-Path $sourceCachePath $sourceId
$archivePath = Join-Path $archiveDir $archiveName
$buildPath = Join-Path $repoRoot $BuildDir
$outFullPath = Join-Path $repoRoot $OutPath

New-Item -ItemType Directory -Force -Path $archiveDir | Out-Null
if (-not (Test-Path -LiteralPath $archivePath)) {
    Invoke-WebRequest -Uri $url -OutFile $archivePath -Headers @{ "User-Agent" = "zeff-romtest" }
}

$actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
if ($actualSha256 -ne $expectedSha256) {
    throw "mGBA suite source hash mismatch: expected $expectedSha256, got $actualSha256"
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "Docker is required to build mGBA suite. Install/start Docker Desktop or use a devkitARM environment manually."
}

$oldErrorActionPreference = $ErrorActionPreference
$ErrorActionPreference = "Continue"
docker image inspect devkitpro/devkitarm *> $null
$imageInspectExitCode = $LASTEXITCODE
$ErrorActionPreference = $oldErrorActionPreference

if ($imageInspectExitCode -ne 0) {
    Write-Host "Pulling devkitpro/devkitarm..."
    docker pull devkitpro/devkitarm
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to pull devkitpro/devkitarm. Is Docker Desktop running?"
    }
}

$resolvedBuildParent = Resolve-Path (Join-Path $repoRoot "rom-tests/cache")
if (Test-Path -LiteralPath $buildPath) {
    $resolvedBuildPath = Resolve-Path $buildPath
    if (-not $resolvedBuildPath.Path.StartsWith($resolvedBuildParent.Path, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove build directory outside rom-tests/cache: $($resolvedBuildPath.Path)"
    }
    Remove-Item -LiteralPath $resolvedBuildPath.Path -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $buildPath | Out-Null
Expand-Archive -LiteralPath $archivePath -DestinationPath $buildPath -Force
$sourcePath = Join-Path $buildPath $archivePrefix

docker run --rm -v "${sourcePath}:/root/suite" -w /root/suite devkitpro/devkitarm make
if ($LASTEXITCODE -ne 0) {
    throw "mGBA suite Docker build failed"
}

$builtRom = Join-Path $sourcePath "suite.gba"
if (-not (Test-Path -LiteralPath $builtRom)) {
    throw "mGBA suite build completed but suite.gba was not produced"
}

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $outFullPath) | Out-Null
Copy-Item -LiteralPath $builtRom -Destination $outFullPath -Force
Write-Host "Wrote $OutPath"
