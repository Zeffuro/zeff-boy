param(
    [string]$SourceCacheDir = "rom-tests/cache/_sources",
    [string]$BuildDir = "rom-tests/cache/_build/coleco-cvbasic-controller-8e5a3a19",
    [string]$OutPath = "rom-tests/cache/coleco/nanochess/cvbasic-controller-8e5a3a19/controller.col",
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$cvCommit = "8e5a3a1905362b90f99e7542d6e0e02cf6f7880f"
$cvSha256 = "2a4fc56b4e0bb3c3be0aa593ffd5ec22a6c1a2392396600ebaf17f2a7472cedf"
$gasmCommit = "3f736b0a9d20f4773e8acbbb6b83517a378d2443"
$gasmSha256 = "bdd991585af4bddce81b649e6e077263435461da75a6277bcb4d4c3d540c4130"
$romSha256 = "aad8e30e3f0ef8171090b6eb9a3c5114dc5987e1e739660ec452e51e648369f4"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cacheRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot "rom-tests/cache"))
$sourceRoot = Join-Path $repoRoot $SourceCacheDir
$buildPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $BuildDir))
$outFullPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutPath))
$cvArchive = Join-Path $sourceRoot "nanochess-cvbasic-controller-8e5a3a19/$cvCommit.zip"
$gasmArchive = Join-Path $sourceRoot "nanochess-gasm80-3f736b0a/$gasmCommit.zip"

function Assert-UnderCache([string]$Path) {
    $prefix = $cacheRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $Path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to write outside rom-tests/cache: $Path"
    }
}

function Invoke-Checked([string]$FilePath, [string[]]$Arguments, [string]$FailureMessage) {
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw $FailureMessage
    }
}

function Get-PinnedArchive([string]$Url, [string]$Path, [string]$ExpectedSha256) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    if (-not (Test-Path -LiteralPath $Path)) {
        Invoke-WebRequest -Uri $Url -OutFile $Path -Headers @{ "User-Agent" = "zeff-romtest" }
    }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    if ($actual -ne $ExpectedSha256) {
        throw "Source hash mismatch for $Path`: expected $ExpectedSha256, got $actual"
    }
}

Assert-UnderCache $buildPath
Assert-UnderCache $outFullPath

if ($DryRun) {
    Write-Host "Would build CVBasic $cvCommit with Gasm80 $gasmCommit"
    Write-Host "Would write $OutPath"
    exit 0
}

$gcc = Get-Command gcc -ErrorAction SilentlyContinue
if (-not $gcc) {
    throw "gcc is required to build the CVBasic controller diagnostic"
}

Get-PinnedArchive "https://github.com/nanochess/CVBasic/archive/$cvCommit.zip" $cvArchive $cvSha256
Get-PinnedArchive "https://github.com/nanochess/gasm80/archive/$gasmCommit.zip" $gasmArchive $gasmSha256

New-Item -ItemType Directory -Force -Path $buildPath | Out-Null
Expand-Archive -LiteralPath $cvArchive -DestinationPath $buildPath -Force
Expand-Archive -LiteralPath $gasmArchive -DestinationPath $buildPath -Force

$cvSource = Join-Path $buildPath "CVBasic-$cvCommit"
$gasmSource = Join-Path $buildPath "gasm80-$gasmCommit"
$cvExe = Join-Path $buildPath "cvbasic-tool.exe"
$gasmExe = Join-Path $buildPath "gasm80-tool.exe"
$asmPath = Join-Path $cvSource "controller.zeff.asm"
$romPath = Join-Path $cvSource "controller.zeff.col"

Push-Location $cvSource
try {
    Invoke-Checked $gcc.Source @("-O", "cvbasic.c", "node.c", "driver.c", "cpuz80.c", "cpu6502.c", "cpu9900.c", "-o", $cvExe) "CVBasic build failed"
} finally {
    Pop-Location
}

Push-Location $gasmSource
try {
    Invoke-Checked $gcc.Source @("-O", "gasm80.c", "-o", $gasmExe) "Gasm80 build failed"
} finally {
    Pop-Location
}

Push-Location $cvSource
try {
    Invoke-Checked $cvExe @("examples/controller.bas", $asmPath) "CVBasic controller compilation failed"
    Invoke-Checked $gasmExe @($asmPath, "-o", $romPath) "CVBasic controller assembly failed"
} finally {
    Pop-Location
}

$actualRomSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $romPath).Hash.ToLowerInvariant()
if ($actualRomSha256 -ne $romSha256) {
    throw "Controller ROM hash mismatch: expected $romSha256, got $actualRomSha256"
}

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $outFullPath) | Out-Null
Copy-Item -LiteralPath $romPath -Destination $outFullPath -Force
Write-Host "Wrote $OutPath"
