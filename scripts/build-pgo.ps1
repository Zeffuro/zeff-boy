param(
    [ValidateRange(1, 1000000)]
    [int]$Frames = 1200,

    [ValidateRange(1, 1000000)]
    [int]$GameplayFrames = 600,

    [ValidateRange(1, 100)]
    [int]$GameplayRomsPerCore = 2,

    [switch]$SkipGameplayTraining
)

$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$sessionRoot = Join-Path $repoRoot "target\pgo\$stamp"
$dataDir = Join-Path $sessionRoot 'data'
$generateTarget = Join-Path $sessionRoot 'generate'
$useTarget = Join-Path $sessionRoot 'use'
$mergedProfile = Join-Path $dataDir 'merged.profdata'
$outputDir = Join-Path $repoRoot 'target\pgo'
$outputExe = Join-Path $outputDir 'zeff-boy.exe'

$targetLibDir = (& rustc --print target-libdir).Trim()
if ($LASTEXITCODE -ne 0) {
    throw 'rustc --print target-libdir failed'
}

$llvmProfdata = Join-Path (Split-Path -Parent $targetLibDir) 'bin\llvm-profdata.exe'
if (-not (Test-Path -LiteralPath $llvmProfdata -PathType Leaf)) {
    throw "llvm-profdata was not found. Run: rustup component add llvm-tools-preview"
}

# Let Cargo create its target roots so it also writes the cache-directory tag
# required by `cargo clean --target-dir`.
New-Item -ItemType Directory -Path $dataDir, $outputDir -Force | Out-Null

$savedEnvironment = @{}
foreach ($name in @(
    'CARGO_TARGET_DIR',
    'LLVM_PROFILE_FILE',
    'RUSTFLAGS',
    'ZEFF_MUTE_AUDIO',
    'ZEFF_PROFILE_AUDIO',
    'ZEFF_PROFILE_CORE',
    'ZEFF_PROFILE_FRAMES',
    'ZEFF_PROFILE_TRACE'
)) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

function Invoke-Cargo {
    param([Parameter(Mandatory)][string[]]$Arguments)

    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') failed"
    }
}

function Invoke-TrainingRun {
    param(
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][int]$TrainingFrames,
        [switch]$Audio,
        [switch]$Trace
    )

    $env:ZEFF_PROFILE_CORE = 'all'
    $env:ZEFF_PROFILE_FRAMES = $TrainingFrames.ToString()
    $env:ZEFF_PROFILE_AUDIO = if ($Audio) { '1' } else { $null }
    $env:ZEFF_PROFILE_TRACE = if ($Trace) { '1' } else { $null }

    & $Executable
    if ($LASTEXITCODE -ne 0) {
        throw "PGO training run failed with exit code $LASTEXITCODE"
    }
}

function Resolve-GameplayRomPath {
    param([Parameter(Mandatory)][string]$RomPath)

    if (Test-Path -LiteralPath $RomPath -PathType Leaf) {
        return (Resolve-Path -LiteralPath $RomPath).Path
    }

    $testRoms = Join-Path $repoRoot 'test-roms'
    $relativeCandidate = Join-Path $testRoms $RomPath
    if (Test-Path -LiteralPath $relativeCandidate -PathType Leaf) {
        return (Resolve-Path -LiteralPath $relativeCandidate).Path
    }

    # Local compatibility manifests may retain the original ROM-library path
    # after selected dumps have been copied under this ignored test-roms tree.
    $romsMarker = $RomPath.IndexOf('\Roms\', [StringComparison]::OrdinalIgnoreCase)
    if ($romsMarker -ge 0) {
        $libraryRelative = $RomPath.Substring($romsMarker + '\Roms\'.Length)
        $localCopy = Join-Path $testRoms $libraryRelative
        if (Test-Path -LiteralPath $localCopy -PathType Leaf) {
            return (Resolve-Path -LiteralPath $localCopy).Path
        }
    }

    return $null
}

function Get-GameplayCorpus {
    $entries = [System.Collections.Generic.List[object]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    $testRoms = Join-Path $repoRoot 'test-roms'
    if (-not (Test-Path -LiteralPath $testRoms -PathType Container)) {
        return @()
    }

    # Existing ignored benchmark manifests are deliberately local-only. Their
    # simple format is label<TAB>path-relative-to-test-roms.
    Get-ChildItem -LiteralPath $testRoms -Filter '*-bench-roms.txt' -File |
        Sort-Object Name |
        ForEach-Object {
            $core = $_.BaseName.Substring(0, $_.BaseName.Length - '-bench-roms'.Length)
            foreach ($line in Get-Content -LiteralPath $_.FullName) {
                if ([String]::IsNullOrWhiteSpace($line) -or $line.TrimStart().StartsWith('#')) {
                    continue
                }
                $parts = $line.Split("`t", 2)
                if ($parts.Count -ne 2) {
                    continue
                }
                $path = Resolve-GameplayRomPath -RomPath $parts[1].Trim()
                if ($null -ne $path -and $seen.Add($path)) {
                    $entries.Add([pscustomobject]@{
                        Core = $core
                        Label = $parts[0].Trim()
                        Path = $path
                    })
                }
            }
        }

    # Generated local compatibility manifests cover additional cores while
    # keeping copyrighted titles, hashes, and paths outside Git.
    $compatRoot = Join-Path $repoRoot 'rom-tests\manifests\compat-games'
    if (Test-Path -LiteralPath $compatRoot -PathType Container) {
        Get-ChildItem -LiteralPath $compatRoot -Filter 'local*.toml' -File |
            Sort-Object Name |
            ForEach-Object {
                $core = $null
                foreach ($line in Get-Content -LiteralPath $_.FullName) {
                    if ($line -match '^\s*core\s*=\s*"(?<core>[^"]+)"') {
                        $core = $Matches.core
                        continue
                    }
                    if ($null -eq $core -or $line -notmatch '^\s*path\s*=\s*(?<path>".*")\s*$') {
                        continue
                    }
                    try {
                        $manifestPath = $Matches.path | ConvertFrom-Json
                    }
                    catch {
                        continue
                    }
                    $path = Resolve-GameplayRomPath -RomPath $manifestPath
                    if ($null -ne $path -and $seen.Add($path)) {
                        $entries.Add([pscustomobject]@{
                            Core = $core
                            Label = [IO.Path]::GetFileNameWithoutExtension($path)
                            Path = $path
                        })
                    }
                }
            }
    }

    $selected = [System.Collections.Generic.List[object]]::new()
    foreach ($group in $entries | Group-Object Core | Sort-Object Name) {
        $ordered = @($group.Group | Sort-Object Path)
        $take = [Math]::Min($GameplayRomsPerCore, $ordered.Count)
        for ($i = 0; $i -lt $take; $i++) {
            $index = if ($take -eq 1) {
                0
            }
            else {
                [Math]::Floor($i * ($ordered.Count - 1) / ($take - 1))
            }
            $selected.Add($ordered[$index])
        }
    }

    return @($selected)
}

function Invoke-GameplayTraining {
    param([Parameter(Mandatory)][string]$Executable)

    $corpus = @(Get-GameplayCorpus)
    if ($corpus.Count -eq 0) {
        Write-Warning 'No ignored local gameplay corpus was found; continuing with portable synthetic training only.'
        return
    }

    # These deterministic pulses aim to pass common title/menu screens and
    # exercise movement/action paths. Per-title replays remain the stronger
    # option when a curated gameplay checkpoint is available.
    $input = @(
        'start@30-31',
        'start@90-91',
        'start@150-151',
        'start@210-211',
        'a@270-299',
        'right+a@300-359',
        'down+b@360-419',
        'left+a@420-479',
        'up+b@480-539',
        'right@540-599'
    ) -join ','

    Write-Output "Training $($corpus.Count) local gameplay ROMs for $GameplayFrames frames each"
    $completed = 0
    foreach ($entry in $corpus) {
        Write-Output "  $($entry.Core): $($entry.Label)"
        & $Executable --headless --max-frames $GameplayFrames --no-sram --press $input $entry.Path
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Skipping failed gameplay training entry: $($entry.Path)"
            continue
        }
        $completed++
    }
    Write-Output "Completed $completed/$($corpus.Count) local gameplay training runs"
}

try {
    $env:CARGO_TARGET_DIR = $generateTarget
    $env:RUSTFLAGS = "-Cprofile-generate=$dataDir"
    $env:LLVM_PROFILE_FILE = Join-Path $dataDir '%m-%p.profraw'
    $env:ZEFF_MUTE_AUDIO = '1'

    Invoke-Cargo @('build', '--profile', 'profiling', '--bin', 'profile_cores', '--features', 'profile-cores')

    $trainingExe = Join-Path $generateTarget 'profiling\profile_cores.exe'
    Invoke-TrainingRun -Executable $trainingExe -TrainingFrames $Frames

    # PCE intentionally bounds queued audio in save states, so keep auxiliary
    # audio/trace training short while still covering those branches.
    $auxiliaryFrames = [Math]::Min($Frames, 100)
    Invoke-TrainingRun -Executable $trainingExe -TrainingFrames $auxiliaryFrames -Audio
    Invoke-TrainingRun -Executable $trainingExe -TrainingFrames $auxiliaryFrames -Trace

    if (-not $SkipGameplayTraining) {
        Invoke-Cargo @('build', '--profile', 'profiling', '--bin', 'zeff-boy')
        $gameplayTrainingExe = Join-Path $generateTarget 'profiling\zeff-boy.exe'
        Invoke-GameplayTraining -Executable $gameplayTrainingExe
    }

    $rawProfiles = @(
        Get-ChildItem -LiteralPath $dataDir -Filter '*.profraw' -File |
            Select-Object -ExpandProperty FullName
    )
    if ($rawProfiles.Count -eq 0) {
        throw 'PGO training produced no .profraw files'
    }

    & $llvmProfdata merge -o $mergedProfile $rawProfiles
    if ($LASTEXITCODE -ne 0) {
        throw 'llvm-profdata merge failed'
    }

    $env:CARGO_TARGET_DIR = $useTarget
    $env:RUSTFLAGS = "-Cprofile-use=$mergedProfile"
    $env:LLVM_PROFILE_FILE = $null
    $env:ZEFF_PROFILE_AUDIO = $null
    $env:ZEFF_PROFILE_CORE = $null
    $env:ZEFF_PROFILE_FRAMES = $null
    $env:ZEFF_PROFILE_TRACE = $null

    Invoke-Cargo @('build', '--release', '--bin', 'zeff-boy')

    $builtExe = Join-Path $useTarget 'release\zeff-boy.exe'
    Copy-Item -LiteralPath $builtExe -Destination $outputExe -Force

    & cargo clean --target-dir $generateTarget
    if ($LASTEXITCODE -ne 0) {
        throw 'cleaning the instrumented PGO target failed'
    }
    & cargo clean --target-dir $useTarget
    if ($LASTEXITCODE -ne 0) {
        throw 'cleaning the optimized PGO target failed'
    }

    Write-Output "PGO build: $outputExe"
    Write-Output "Training data: $sessionRoot"
}
finally {
    foreach ($entry in $savedEnvironment.GetEnumerator()) {
        [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, 'Process')
    }
}
