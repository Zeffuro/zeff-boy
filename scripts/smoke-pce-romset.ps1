[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string[]]$RomRoots,
    [string]$ZeffBoy = "target\release\zeff-boy.exe",
    [string]$OutputDirectory = "tmp\pce-romset-smoke",
    [int]$Frames = 600,
    [int]$TimeoutSeconds = 300,
    [int]$Limit = 12,
    [switch]$All,
    [string[]]$NameMatch = @(),
    [string[]]$Input = @(),
    [switch]$Resume,
    [switch]$ApplyMods,
    [switch]$WithAudio,
    [switch]$NoDebugState,
    [switch]$DetectStuck,
    [switch]$FailOnStuck,
    [switch]$DryRun,
    [switch]$ShowPaths
)

$ErrorActionPreference = "Stop"

if ($Frames -le 0) {
    throw "Frames must be positive."
}
if ($TimeoutSeconds -le 0) {
    throw "TimeoutSeconds must be positive."
}
if ($Limit -lt 0) {
    throw "Limit must be zero or positive."
}
if ($FailOnStuck -and -not $DetectStuck) {
    throw "FailOnStuck requires DetectStuck."
}
if ($All) {
    $Limit = 0
}

$supportedExtensions = @(".pce", ".zip", ".7z", ".cue", ".chd")
$resolvedRoots = @(
    $RomRoots |
        ForEach-Object { [System.IO.Path]::GetFullPath($_) } |
        Sort-Object -Unique
)
$resolvedOutput = [System.IO.Path]::GetFullPath($OutputDirectory)
$resolvedExecutable = [System.IO.Path]::GetFullPath($ZeffBoy)
$writeDebugState = -not $NoDebugState

if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "Zeff Boy executable was not found: $resolvedExecutable. Build it once first, for example: cargo build --release"
}
$executableInfo = Get-Item -LiteralPath $resolvedExecutable

$items = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
foreach ($root in $resolvedRoots) {
    if (-not (Test-Path -LiteralPath $root -PathType Container)) {
        throw "ROM root was not found: $root"
    }
    Get-ChildItem -LiteralPath $root -File -Recurse |
        Where-Object {
            $supportedExtensions -contains $_.Extension.ToLowerInvariant() -and
            -not $_.Name.StartsWith("[BIOS]", [System.StringComparison]::OrdinalIgnoreCase)
        } |
        ForEach-Object { $items.Add($_) }
}

$normalizedMatches = @(
    $NameMatch |
        ForEach-Object { $_.Trim().ToLowerInvariant() } |
        Where-Object { $_.Length -gt 0 }
)
if ($normalizedMatches.Count -gt 0) {
    $filteredItems = $items | Where-Object {
        $name = $_.Name.ToLowerInvariant()
        $normalizedMatches | Where-Object { $name.Contains($_) } | Select-Object -First 1
    }
    $items = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
    foreach ($filteredItem in @($filteredItems)) {
        $items.Add($filteredItem)
    }
}

$sortedItems = $items | Sort-Object FullName -Unique
$items = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
foreach ($sortedItem in @($sortedItems)) {
    $items.Add($sortedItem)
}
if ($Limit -gt 0) {
    $limitedItems = $items | Select-Object -First $Limit
    $items = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
    foreach ($limitedItem in @($limitedItems)) {
        $items.Add($limitedItem)
    }
}
if ($items.Count -eq 0) {
    throw "No PCE media files matched the selected roots and filters."
}

if (-not $DryRun) {
    New-Item -ItemType Directory -Force -Path $resolvedOutput | Out-Null
}
$reportPath = Join-Path $resolvedOutput "report.json"
$logDirectory = Join-Path $resolvedOutput "logs"
$stateDirectory = Join-Path $resolvedOutput "states"
if (-not $DryRun) {
    New-Item -ItemType Directory -Force -Path $logDirectory | Out-Null
    if ($writeDebugState) {
        New-Item -ItemType Directory -Force -Path $stateDirectory | Out-Null
    }
}

function Get-ItemKey([System.IO.FileInfo]$Item) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($Item.FullName)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $algorithm.ComputeHash($bytes)
    } finally {
        $algorithm.Dispose()
    }
    return ([BitConverter]::ToString($hash).Replace("-", "").ToLowerInvariant()).Substring(0, 16)
}

function Write-Report($Report) {
    $tempPath = "$reportPath.tmp"
    $Report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $tempPath -Encoding utf8
    Move-Item -LiteralPath $tempPath -Destination $reportPath -Force
}

$runSignature = @(
    "exe_path=$resolvedExecutable"
    "exe_size=$($executableInfo.Length)"
    "exe_mtime=$($executableInfo.LastWriteTimeUtc.Ticks)"
    "frames=$Frames"
    "timeout_seconds=$TimeoutSeconds"
    "no_apu=$(-not $WithAudio)"
    "apply_mods=$([bool]$ApplyMods)"
    "detect_stuck=$([bool]$DetectStuck)"
    "fail_on_stuck=$([bool]$FailOnStuck)"
    "debug_state=$writeDebugState"
    "input=$($Input -join [char]0x1F)"
) -join [char]0x1E

$previousByKey = @{}
if ($Resume -and (Test-Path -LiteralPath $reportPath -PathType Leaf)) {
    $previous = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    foreach ($entry in @($previous.items)) {
        $previousByKey[$entry.key] = $entry
    }
}

$report = [ordered]@{
    schema_version = 1
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
    executable = $resolvedExecutable
    roots = $resolvedRoots
    frames = $Frames
    no_sram = $true
    timeout_seconds = $TimeoutSeconds
    no_apu = -not $WithAudio
    apply_mods = [bool]$ApplyMods
    detect_stuck = [bool]$DetectStuck
    input = $Input
    run_signature = $runSignature
    items = [System.Collections.Generic.List[object]]::new()
}

$estimatedFrames = [int64]$items.Count * $Frames
Write-Host "Selected $($items.Count) PCE items ($estimatedFrames emulated frames)."
Write-Host "One process is used per title. Archives are decoded once per attempted title; -Resume skips unchanged completed items."
if ($ShowPaths) {
    $items | ForEach-Object { Write-Host $_.FullName }
}

foreach ($item in $items) {
    $key = Get-ItemKey $item
    $signature = "$($item.Length):$($item.LastWriteTimeUtc.Ticks)"
    $existing = $previousByKey[$key]
    if ($Resume -and $null -ne $existing -and $existing.signature -eq $signature -and $existing.run_signature -eq $runSignature -and $existing.status -eq "passed") {
        $report.items.Add($existing)
        Write-Host "[skip] $($item.Name)"
        continue
    }

    $entry = [pscustomobject][ordered]@{
        key = $key
        path = $item.FullName
        extension = $item.Extension.ToLowerInvariant()
        signature = $signature
        run_signature = $runSignature
        status = "running"
        exit_code = $null
        elapsed_ms = $null
        timed_out = $false
        summary = $null
        log = $null
        stderr_log = $null
        debug_state = $null
        debug = $null
    }
    $report.items.Add($entry)
    if (-not $DryRun) {
        Write-Report $report
    }

    $args = @("--headless", "--max-frames", "$Frames", "--no-sram")
    if (-not $WithAudio) { $args += "--no-apu" }
    if ($ApplyMods) { $args += "--apply-mods" }
    if ($DetectStuck) { $args += "--detect-stuck" }
    if ($FailOnStuck) { $args += "--fail-on-stuck" }
    foreach ($inputEvent in $Input) { $args += @("--press", $inputEvent) }
    $logPath = Join-Path $logDirectory "$key.log"
    $stderrPath = Join-Path $logDirectory "$key.stderr.log"
    $statePath = Join-Path $stateDirectory "$key.json"
    if ($writeDebugState) { $args += @("--debug-state-out", $statePath) }
    $args += $item.FullName

    if ($DryRun) {
        $entry.status = "planned"
        $entry.summary = "& $resolvedExecutable $($args -join ' ')"
        continue
    }

    Write-Host "[run] $($item.Name)"
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $argumentLine = (($args | ForEach-Object { '"' + $_.Replace('"', '\"') + '"' }) -join " ")
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $resolvedExecutable
    $startInfo.Arguments = $argumentLine
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Could not start Zeff Boy for $($item.FullName)"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
    if ($timedOut) {
        $process.Refresh()
        if (-not $process.HasExited) {
            $taskkill = Join-Path $env:WINDIR "System32\taskkill.exe"
            $savedErrorActionPreference = $ErrorActionPreference
            try {
                $ErrorActionPreference = "Continue"
                & $taskkill /PID "$($process.Id)" /T /F *> $null
            } finally {
                $ErrorActionPreference = $savedErrorActionPreference
            }
            $process.WaitForExit()
        }
        $exitCode = $null
    } else {
        $process.WaitForExit()
        $exitCode = $process.ExitCode
    }
    $stopwatch.Stop()
    $stdoutTask.GetAwaiter().GetResult() | Set-Content -LiteralPath $logPath -Encoding utf8
    $stderrTask.GetAwaiter().GetResult() | Set-Content -LiteralPath $stderrPath -Encoding utf8
    $log = if (Test-Path -LiteralPath $logPath) { Get-Content -LiteralPath $logPath -Raw } else { "" }
    $summary = [regex]::Match($log, "(?m)^\[headless\] system=pce frames=.+$").Value

    $entry.status = if ($timedOut) { "timed_out" } elseif ($exitCode -eq 0) { "passed" } else { "failed" }
    $entry.exit_code = $exitCode
    $entry.elapsed_ms = $stopwatch.ElapsedMilliseconds
    $entry.timed_out = $timedOut
    $entry.summary = if ([string]::IsNullOrEmpty($summary)) { $null } else { $summary }
    $entry.log = $logPath
    if ((Get-Item -LiteralPath $stderrPath).Length -gt 0) { $entry.stderr_log = $stderrPath }
    if ($writeDebugState -and (Test-Path -LiteralPath $statePath)) {
        $entry.debug_state = $statePath
        $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
        $entry.debug = [ordered]@{
            frames = $state.frames
            pc = $state.pc_hex
            framebuffer = $state.framebuffer.fingerprint
            topology = $state.hardware.topology
            controller = $state.hardware.controller_mode
            cdrom2 = $state.cdrom2.present
            stuck = $state.stuck
        }
    }
    Write-Report $report
}

if (-not $DryRun) {
    Write-Report $report
}
$passed = @($report.items | Where-Object { $_.status -eq "passed" }).Count
$failed = @($report.items | Where-Object { $_.status -in "failed", "timed_out" }).Count
$skipped = @($report.items | Where-Object { $_.status -eq "planned" }).Count
Write-Host "Finished: passed=$passed failed=$failed planned=$skipped report=$reportPath"
if ($failed -gt 0) { exit 1 }
