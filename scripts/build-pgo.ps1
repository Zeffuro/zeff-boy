param(
    [ValidateRange(1, 1000000)]
    [int]$Frames = 1200,

    [ValidateRange(1, 1000000)]
    [int]$GameplayFrames = 600,

    [ValidateRange(1, 100)]
    [int]$GameplayRomsPerCore = 2,

    [switch]$ListGameplayCorpus,

    [switch]$SkipGameplayTraining
)

$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$sessionRoot = Join-Path $repoRoot "local-artifacts\pgo\$stamp"
$dataDir = Join-Path $sessionRoot 'data'
$generateTarget = Join-Path $sessionRoot 'generate'
$useTarget = Join-Path $sessionRoot 'use'
$mergedProfile = Join-Path $dataDir 'merged.profdata'
$outputDir = Join-Path $repoRoot 'local-artifacts\pgo'
$outputExe = Join-Path $outputDir 'zeff-boy.exe'

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

    $repoCandidate = Join-Path $repoRoot $RomPath
    if (Test-Path -LiteralPath $repoCandidate -PathType Leaf) {
        return (Resolve-Path -LiteralPath $repoCandidate).Path
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

function Add-GameplayCorpusEntry {
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][System.Collections.Generic.List[object]]$Entries,
        [Parameter(Mandatory)][AllowEmptyCollection()][System.Collections.Generic.HashSet[string]]$Seen,
        [Parameter(Mandatory)][string]$Core,
        [Parameter(Mandatory)][string]$Label,
        [Parameter(Mandatory)][string]$RomPath,
        [Nullable[int]]$MaxFrames,
        [AllowEmptyString()][string]$Input,
        [Parameter(Mandatory)][string]$InputSource,
        [Parameter(Mandatory)][int]$SourcePriority
    )

    $path = Resolve-GameplayRomPath -RomPath $RomPath
    if ($null -eq $path -or -not $Seen.Add($path)) {
        return
    }

    $Entries.Add([pscustomobject]@{
        Core = $Core
        Label = $Label
        Path = $path
        MaxFrames = $MaxFrames
        Input = $Input
        InputSource = $InputSource
        SourcePriority = $SourcePriority
    })
}

function ConvertFrom-ManifestInput {
    param([Parameter(Mandatory)][string]$Value)

    try {
        $parsed = ConvertFrom-Json -InputObject ($Value -replace ',\s*\]$', ']')
        return @($parsed) -join ','
    }
    catch {
        return $null
    }
}

function Get-GameplayTrainingFrames {
    param(
        [Parameter(Mandatory)]$Entry,
        [Parameter(Mandatory)][int]$RequestedFrames
    )

    if ($Entry.SourcePriority -eq 0 -and $Entry.InputSource -eq 'manifest' -and $null -ne $Entry.MaxFrames) {
        return [int]$Entry.MaxFrames
    }
    if ($null -eq $Entry.MaxFrames) {
        return $RequestedFrames
    }
    return [Math]::Min($RequestedFrames, [int]$Entry.MaxFrames)
}

function Select-GameplayCorpusEntries {
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][System.Collections.Generic.List[object]]$Entries,
        [Parameter(Mandatory)][int]$RomsPerCore
    )

    $selected = [System.Collections.Generic.List[object]]::new()
    foreach ($coreGroup in $Entries | Group-Object Core | Sort-Object Name) {
        $remaining = [Math]::Min($RomsPerCore, $coreGroup.Count)
        foreach ($priorityGroup in $coreGroup.Group | Group-Object SourcePriority | Sort-Object Name) {
            if ($remaining -eq 0) {
                break
            }
            # Keep real benchmark/compat entries ahead of diagnostics. Where
            # only diagnostics exist, prefer the longest declared coverage
            # before supplementing it with short hardware probes.
            $ordered = @($priorityGroup.Group | Sort-Object @{ Expression = {
                        if ($null -eq $_.MaxFrames) { [int]::MaxValue } else { -[int]$_.MaxFrames }
                    }
                }, Path)
            $take = [Math]::Min($remaining, $ordered.Count)
            for ($i = 0; $i -lt $take; $i++) {
                $index = if ($take -eq 1) {
                    0
                }
                else {
                    [Math]::Floor($i * ($ordered.Count - 1) / ($take - 1))
                }
                $selected.Add($ordered[$index])
            }
            $remaining -= $take
        }
    }

    return @($selected)
}

function Test-GameplayCorpusSelectionPriorities {
    $entries = [System.Collections.Generic.List[object]]@(
        [pscustomobject]@{ Core = 'test'; Label = 'real-a'; Path = 'a'; MaxFrames = $null; SourcePriority = 0 },
        [pscustomobject]@{ Core = 'test'; Label = 'real-b'; Path = 'b'; MaxFrames = $null; SourcePriority = 0 },
        [pscustomobject]@{ Core = 'test'; Label = 'diagnostic'; Path = 'c'; MaxFrames = 600; SourcePriority = 1 }
    )
    $selected = @(Select-GameplayCorpusEntries -Entries $entries -RomsPerCore 2)
    $labels = @($selected | ForEach-Object Label)
    if ($labels.Count -ne 2 -or $labels -notcontains 'real-a' -or $labels -notcontains 'real-b') {
        throw 'PGO corpus selection did not exhaust the highest-priority entries first.'
    }

    $parsedInput = ConvertFrom-ManifestInput -Value "[`n`"start@10-11`",`n`"right@20-30`",`n]"
    if ($parsedInput -ne 'start@10-11,right@20-30') {
        throw 'PGO corpus input parser did not preserve a multiline TOML schedule.'
    }
    $manifestEntry = [pscustomobject]@{ MaxFrames = 3000; InputSource = 'manifest'; SourcePriority = 0 }
    $diagnosticEntry = [pscustomobject]@{ MaxFrames = 220000; InputSource = 'manifest'; SourcePriority = 1 }
    if ((Get-GameplayTrainingFrames -Entry $manifestEntry -RequestedFrames 600) -ne 3000 -or
        (Get-GameplayTrainingFrames -Entry $diagnosticEntry -RequestedFrames 600) -ne 600) {
        throw 'PGO corpus training frame selection did not separate gameplay from diagnostics.'
    }
}

function Add-ManifestGameplayCorpus {
    param(
        [Parameter(Mandatory)][string]$ManifestPath,
        [Parameter(Mandatory)][AllowEmptyCollection()][System.Collections.Generic.List[object]]$Entries,
        [Parameter(Mandatory)][AllowEmptyCollection()][System.Collections.Generic.HashSet[string]]$Seen,
        [Parameter(Mandatory)][bool]$UseGenericInput,
        [Parameter(Mandatory)][int]$SourcePriority
    )

    $test = $null
    $group = $null
    $groupRom = $null
    $section = $null
    $inputLines = $null
    $inputOwner = $null

    foreach ($line in Get-Content -LiteralPath $ManifestPath) {
        if ($null -ne $inputLines) {
            $inputLines.Add($line.Trim())
            if ($line -match '\]\s*$') {
                $input = ConvertFrom-ManifestInput -Value ($inputLines -join '')
                if ($null -eq $input) {
                    throw "Invalid input schedule in $ManifestPath"
                }
                $inputOwner.Input = $input
                $inputOwner.InputSource = 'manifest'
                $inputLines = $null
                $inputOwner = $null
            }
            continue
        }
        if ($line -match '^\s*\[\[(?<section>[^\]]+)\]\]\s*$') {
            if ($null -ne $test -and $null -ne $test.Core -and $null -ne $test.Path -and $test.ArtifactKind -ne 'firmware') {
                Add-GameplayCorpusEntry -Entries $Entries -Seen $Seen -Core $test.Core `
                    -Label $test.Label -RomPath $test.Path -MaxFrames $test.MaxFrames `
                    -Input $test.Input -InputSource $test.InputSource -SourcePriority $SourcePriority
            }
            if ($null -ne $groupRom -and $null -ne $groupRom.Core -and $null -ne $groupRom.Path) {
                Add-GameplayCorpusEntry -Entries $Entries -Seen $Seen -Core $groupRom.Core `
                    -Label $groupRom.Label -RomPath $groupRom.Path -MaxFrames $groupRom.MaxFrames `
                    -Input $groupRom.Input -InputSource $groupRom.InputSource -SourcePriority $SourcePriority
            }
            $test = $null
            $groupRom = $null
            $section = $Matches.section
            switch ($section) {
                'tests' {
                    $test = [pscustomobject]@{
                        Core = $null; Label = $null; Path = $null; MaxFrames = $null
                        Input = $null; InputSource = if ($UseGenericInput) { 'generic' } else { 'none' }; ArtifactKind = $null
                    }
                }
                'test_groups' {
                    $group = [pscustomobject]@{
                        Core = $null; CachePrefix = $null; MaxFrames = $null
                        Input = $null; InputSource = 'none'
                    }
                }
                'test_groups.roms' {
                    if ($null -ne $group) {
                        $groupRom = [pscustomobject]@{
                            Core = $group.Core; Label = $null; Path = $null; MaxFrames = $group.MaxFrames
                            Input = $group.Input; InputSource = $group.InputSource
                            CachePrefix = $group.CachePrefix
                        }
                    }
                }
            }
            continue
        }
        if ($line -match '^\s*\[(?<section>[^\]]+)\]\s*$') {
            $section = $Matches.section
            continue
        }
        if ($line -match '^\s*kind\s*=\s*"(?<value>[^"]+)"\s*$') {
            if ($null -ne $test -and $section -eq 'tests.artifact') {
                $test.ArtifactKind = $Matches.value
            }
            continue
        }
        if ($line -match '^\s*id\s*=\s*"(?<value>[^"]+)"\s*$') {
            if ($null -ne $test -and $section -eq 'tests') {
                $test.Label = $Matches.value
            }
            elseif ($null -ne $groupRom -and $section -eq 'test_groups.roms') {
                $groupRom.Label = $Matches.value
            }
            continue
        }
        if ($line -match '^\s*core\s*=\s*"(?<value>[^"]+)"\s*$') {
            if ($null -ne $test) {
                $test.Core = $Matches.value
            }
            elseif ($null -ne $group -and $null -eq $groupRom) {
                $group.Core = $Matches.value
            }
            continue
        }
        if ($line -match '^\s*max_frames\s*=\s*(?<value>\d+)\s*$') {
            $maxFrames = [int]$Matches.value
            if ($null -ne $test) {
                $test.MaxFrames = $maxFrames
            }
            elseif ($null -ne $group -and $null -eq $groupRom) {
                $group.MaxFrames = $maxFrames
            }
            continue
        }
        if ($line -match '^\s*input\s*=\s*(?<value>\[.*)$') {
            $inputValue = $Matches.value
            $inputOwner = if ($null -ne $test) {
                $test
            }
            elseif ($null -ne $group -and $null -eq $groupRom) {
                $group
            }
            if ($null -ne $inputOwner) {
                if ($inputValue -match '\]\s*$') {
                    $input = ConvertFrom-ManifestInput -Value $inputValue
                    if ($null -eq $input) {
                        throw "Invalid input schedule in $ManifestPath"
                    }
                    $inputOwner.Input = $input
                    $inputOwner.InputSource = 'manifest'
                    $inputOwner = $null
                }
                else {
                    $inputLines = [System.Collections.Generic.List[string]]::new()
                    $inputLines.Add($inputValue.Trim())
                }
            }
            continue
        }
        if ($line -match '^\s*cache_prefix\s*=\s*"(?<value>[^"]+)"\s*$' -and $null -ne $group) {
            $group.CachePrefix = $Matches.value
            continue
        }
        if ($line -match '^\s*path\s*=\s*"(?<value>[^"]+)"\s*$' -and $null -ne $test -and $section -eq 'tests.rom') {
            $test.Path = $Matches.value
            continue
        }
        if ($line -match '^\s*archive_path\s*=\s*"(?<value>[^"]+)"\s*$' -and $null -ne $groupRom) {
            if ($null -ne $groupRom.CachePrefix) {
                $groupRom.Path = Join-Path $groupRom.CachePrefix $Matches.value
            }
        }
    }

    if ($null -ne $inputLines) {
        throw "Unterminated input schedule in $ManifestPath"
    }

    if ($null -ne $test -and $null -ne $test.Core -and $null -ne $test.Path -and $test.ArtifactKind -ne 'firmware') {
        Add-GameplayCorpusEntry -Entries $Entries -Seen $Seen -Core $test.Core `
            -Label $test.Label -RomPath $test.Path -MaxFrames $test.MaxFrames `
            -Input $test.Input -InputSource $test.InputSource -SourcePriority $SourcePriority
    }
    if ($null -ne $groupRom -and $null -ne $groupRom.Core -and $null -ne $groupRom.Path) {
        Add-GameplayCorpusEntry -Entries $Entries -Seen $Seen -Core $groupRom.Core `
            -Label $groupRom.Label -RomPath $groupRom.Path -MaxFrames $groupRom.MaxFrames `
            -Input $groupRom.Input -InputSource $groupRom.InputSource -SourcePriority $SourcePriority
    }
}

function Get-GameplayCorpus {
    $entries = [System.Collections.Generic.List[object]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    $testRoms = Join-Path $repoRoot 'test-roms'

    # Existing ignored benchmark manifests are deliberately local-only. Their
    # simple format is label<TAB>path-relative-to-test-roms.
    if (Test-Path -LiteralPath $testRoms -PathType Container) {
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
                    Add-GameplayCorpusEntry -Entries $entries -Seen $seen -Core $core `
                        -Label $parts[0].Trim() -RomPath $parts[1].Trim() -InputSource 'generic' -SourcePriority 0
                }
            }
    }

    # Local compatibility manifests cover user-owned games while retaining
    # their paths outside Git. They receive the generic gameplay schedule.
    $compatRoot = Join-Path $repoRoot 'rom-tests\manifests\compat-games'
    if (Test-Path -LiteralPath $compatRoot -PathType Container) {
        Get-ChildItem -LiteralPath $compatRoot -Filter 'local*.toml' -File |
            Sort-Object Name |
            ForEach-Object {
                Add-ManifestGameplayCorpus -ManifestPath $_.FullName -Entries $entries -Seen $seen -UseGenericInput $true -SourcePriority 0
            }
    }

    # Source-backed and generated local manifest fixtures cover systems that
    # do not yet have a checked local commercial-game corpus. Their declared
    # frame caps and input schedules are preserved; absent input stays absent.
    $testManifestRoot = Join-Path $repoRoot 'rom-tests\manifests\test-roms'
    if (Test-Path -LiteralPath $testManifestRoot -PathType Container) {
        $manifestEntries = [System.Collections.Generic.List[object]]::new()
        $manifestSeen = [System.Collections.Generic.HashSet[string]]::new(
            [StringComparer]::OrdinalIgnoreCase
        )
        Get-ChildItem -LiteralPath $testManifestRoot -Filter '*.toml' -File |
            Sort-Object Name |
            ForEach-Object {
                Add-ManifestGameplayCorpus -ManifestPath $_.FullName -Entries $manifestEntries -Seen $manifestSeen -UseGenericInput $false -SourcePriority 1
            }
        foreach ($entry in $manifestEntries | Where-Object { $_.Core -in @('sms', 'gg', 'sg', 'pce', 'ws') }) {
            Add-GameplayCorpusEntry -Entries $entries -Seen $seen -Core $entry.Core `
                -Label $entry.Label -RomPath $entry.Path -MaxFrames $entry.MaxFrames `
                -Input $entry.Input -InputSource $entry.InputSource -SourcePriority $entry.SourcePriority
        }
    }

    return @(Select-GameplayCorpusEntries -Entries $entries -RomsPerCore $GameplayRomsPerCore)
}

function Show-GameplayCorpus {
    $corpus = @(Get-GameplayCorpus)
    if ($corpus.Count -eq 0) {
        Write-Output 'No ignored local PGO corpus is available.'
        return
    }

    Write-Output "Selected $($corpus.Count) deterministic local PGO entries:"
    foreach ($entry in $corpus) {
        $entryFrames = Get-GameplayTrainingFrames -Entry $entry -RequestedFrames $GameplayFrames
        Write-Output "  $($entry.Core): $($entry.Label) (frames: $entryFrames; input: $($entry.InputSource))"
    }
}

function Invoke-GameplayTraining {
    param([Parameter(Mandatory)][string]$Executable)

    $corpus = @(Get-GameplayCorpus)
    if ($corpus.Count -eq 0) {
        Write-Warning 'No ignored local gameplay corpus was found; continuing with portable synthetic training only.'
        return
    }

    # These deterministic pulses aim to pass common title/menu screens and
    # exercise movement/action paths. Manifest-defined schedules override
    # them; diagnostic fixtures without an input declaration remain idle.
    $genericInput = @(
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

    Write-Output "Training $($corpus.Count) local deterministic ROM entries"
    $completed = 0
    foreach ($entry in $corpus) {
        $entryFrames = Get-GameplayTrainingFrames -Entry $entry -RequestedFrames $GameplayFrames
        $input = if ($entry.InputSource -eq 'manifest') { $entry.Input } elseif ($entry.InputSource -eq 'generic') { $genericInput } else { $null }
        Write-Output "  $($entry.Core): $($entry.Label) ($entryFrames frames; input: $($entry.InputSource))"
        $arguments = @('--headless', '--max-frames', $entryFrames, '--no-sram')
        if (-not [String]::IsNullOrWhiteSpace($input)) {
            $arguments += @('--press', $input)
        }
        $arguments += $entry.Path
        & $Executable @arguments
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Skipping failed gameplay training entry: $($entry.Core) / $($entry.Label)"
            continue
        }
        $completed++
    }
    Write-Output "Completed $completed/$($corpus.Count) local gameplay training runs"
}

if ($ListGameplayCorpus) {
    Test-GameplayCorpusSelectionPriorities
    Show-GameplayCorpus
    exit 0
}

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

try {
    $env:CARGO_TARGET_DIR = $generateTarget
    $env:RUSTFLAGS = "-Cprofile-generate=$dataDir"
    $env:LLVM_PROFILE_FILE = Join-Path $dataDir '%m-%p.profraw'
    $env:ZEFF_MUTE_AUDIO = '1'

    Invoke-Cargo @('build', '--profile', 'profiling', '--bin', 'profile_cores', '--features', 'profile-cores', '--jobs', '1')

    $trainingExe = Join-Path $generateTarget 'profiling\profile_cores.exe'
    Invoke-TrainingRun -Executable $trainingExe -TrainingFrames $Frames

    # PCE intentionally bounds queued audio in save states, so keep auxiliary
    # audio/trace training short while still covering those branches.
    $auxiliaryFrames = [Math]::Min($Frames, 100)
    Invoke-TrainingRun -Executable $trainingExe -TrainingFrames $auxiliaryFrames -Audio
    Invoke-TrainingRun -Executable $trainingExe -TrainingFrames $auxiliaryFrames -Trace

    if (-not $SkipGameplayTraining) {
        Invoke-Cargo @('build', '--profile', 'profiling', '--bin', 'zeff-boy', '--jobs', '1')
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

    Invoke-Cargo @('build', '--release', '--bin', 'zeff-boy', '--jobs', '1')

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
