param(
    [Parameter(Mandatory = $true)]
    [string]$RomPath,

    [string]$RepoRoot = "F:\Coding\zeff-boy",
    [uint64]$MaxFrames = 2400,
    [uint64]$TraceMaxOps = 4000,
    [uint64]$TraceStartT = 0,
    [string]$TracePcRange,
    [string]$TraceOpcode,
    [switch]$TraceWatchInterrupts,
    [switch]$StrictRawCompare,
    [string]$OutputDir = "F:\Coding\zeff-boy\temp\interrupt-trace"
)

$ErrorActionPreference = "Stop"

function Invoke-TraceRun {
    param(
        [string]$Mode,
        [string]$LogPath
    )

    $args = @(
        "run", "--", "--headless",
        "--mode", $Mode,
        "--max-frames", $MaxFrames,
        "--trace-opcodes",
        "--trace-max-ops", $TraceMaxOps,
        "--trace-start-t", $TraceStartT
    )

    if ($TracePcRange) {
        $args += @("--trace-pc-range", $TracePcRange)
    }
    if ($TraceOpcode) {
        $args += @("--trace-opcode", $TraceOpcode)
    }
    if ($TraceWatchInterrupts) {
        $args += "--trace-watch-interrupts"
    }

    $args += $RomPath

    Push-Location $RepoRoot
    try {
        # Windows PowerShell can promote native stderr to ErrorRecord when
        # ErrorActionPreference=Stop. Use Start-Process with redirected streams
        # so warnings are logged but do not terminate the script.
        $stdoutPath = "$LogPath.stdout"
        $stderrPath = "$LogPath.stderr"

        $process = Start-Process `
            -FilePath "cargo" `
            -ArgumentList $args `
            -RedirectStandardOutput $stdoutPath `
            -RedirectStandardError $stderrPath `
            -NoNewWindow `
            -Wait `
            -PassThru

        $stdout = if (Test-Path $stdoutPath) { Get-Content -Path $stdoutPath } else { @() }
        $stderr = if (Test-Path $stderrPath) { Get-Content -Path $stderrPath } else { @() }
        @($stdout + $stderr) | Out-File -FilePath $LogPath -Encoding utf8

        Remove-Item -Path $stdoutPath -ErrorAction SilentlyContinue
        Remove-Item -Path $stderrPath -ErrorAction SilentlyContinue

        if ($process.ExitCode -ne 0) {
            throw "cargo run failed for mode '$Mode'. See $LogPath"
        }
    }
    finally {
        Pop-Location
    }
}

function Get-TraceLines {
    param([string]$Path)

    Get-Content -Path $Path | Where-Object { $_ -match '^\[op\] ' }
}

function Normalize-TraceLine {
    param([string]$Line)

    if ($StrictRawCompare) {
        return $Line.Trim()
    }

    # DMG/CGB mode labels are expected to differ; ignore them for semantic trace diff.
    ($Line -replace '\s+mode=\S+', '').Trim()
}

function Show-TraceContext {
    param(
        [string[]]$Lines,
        [int]$Index,
        [string]$Label
    )

    $start = [Math]::Max(0, $Index - 3)
    $end = [Math]::Min($Lines.Count - 1, $Index + 3)
    Write-Host "`n$Label context [$start..$end]:"
    for ($i = $start; $i -le $end; $i++) {
        $marker = if ($i -eq $Index) { '>>' } else { '  ' }
        Write-Host ("{0} {1}: {2}" -f $marker, $i, $Lines[$i])
    }
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$dmgLog = Join-Path $OutputDir "dmg-trace.log"
$cgbLog = Join-Path $OutputDir "cgb-trace.log"

Write-Host "Running DMG trace..."
Invoke-TraceRun -Mode "dmg" -LogPath $dmgLog
Write-Host "Running CGB trace..."
Invoke-TraceRun -Mode "cgb" -LogPath $cgbLog

$dmgLines = Get-TraceLines -Path $dmgLog
$cgbLines = Get-TraceLines -Path $cgbLog

if ($dmgLines.Count -eq 0 -or $cgbLines.Count -eq 0) {
    throw "No [op] trace lines found. Check flags or logs in $OutputDir"
}

$limit = [Math]::Min($dmgLines.Count, $cgbLines.Count)
$mismatch = -1
$ignoredOnlyDiffs = 0
$firstIgnoredOnly = -1

for ($i = 0; $i -lt $limit; $i++) {
    if ($dmgLines[$i] -ne $cgbLines[$i]) {
        $ignoredOnlyDiffs++
        if ($firstIgnoredOnly -lt 0) {
            $firstIgnoredOnly = $i
        }
    }

    $dmgNorm = Normalize-TraceLine -Line $dmgLines[$i]
    $cgbNorm = Normalize-TraceLine -Line $cgbLines[$i]
    if ($dmgNorm -ne $cgbNorm) {
        $mismatch = $i
        break
    }
}

Write-Host "`nTrace files:"
Write-Host "  DMG: $dmgLog"
Write-Host "  CGB: $cgbLog"
Write-Host "  DMG lines: $($dmgLines.Count)"
Write-Host "  CGB lines: $($cgbLines.Count)"
if (-not $StrictRawCompare) {
    Write-Host "  Ignored-only raw diffs (e.g. mode label): $ignoredOnlyDiffs"
}

if ($mismatch -ge 0) {
    Write-Host "`nFirst mismatch at trace index $mismatch"
    Show-TraceContext -Lines $dmgLines -Index $mismatch -Label "DMG"
    Show-TraceContext -Lines $cgbLines -Index $mismatch -Label "CGB"
    exit 2
}

if ($dmgLines.Count -ne $cgbLines.Count) {
    Write-Host "`nNo line mismatch in shared prefix; lengths differ after index $limit"
    exit 3
}

if (-not $StrictRawCompare -and $ignoredOnlyDiffs -gt 0 -and $firstIgnoredOnly -ge 0) {
    Write-Host "`nRaw logs differ at index $firstIgnoredOnly, but only in ignored fields."
    Show-TraceContext -Lines $dmgLines -Index $firstIgnoredOnly -Label "DMG (raw)"
    Show-TraceContext -Lines $cgbLines -Index $firstIgnoredOnly -Label "CGB (raw)"
}

Write-Host "`nNo mismatch found across compared trace lines."

