param(
    [string]$RomRoot = "Z:\Android\Roms",
    [string[]]$SystemDirs = @("."),
    [string[]]$Extensions = @(),
    [int]$Limit = 0,
    [switch]$SkipArchives,
    [switch]$Probe,
    [switch]$ProbeOnly,
    [int]$ProbeMaxInstructions = 100000,
    [int]$ProbeFrames = 0,
    [string[]]$ProbeExtensions = @(),
    [switch]$ProbeShowPaths,
    [string]$IssueReport = "",
    [switch]$IssueReportPaths,
    [switch]$ShowRootPath
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$supportedExtensions = @(".sms", ".gg", ".sg", ".sc")
$headerOffsets = @(0x7FF0, 0x3FF0, 0x1FF0)
$headerMagic = [System.Text.Encoding]::ASCII.GetBytes("TMR SEGA")
$romBankSize = 0x4000
$copierHeaderSize = 512

function Normalize-Extension([string]$Extension) {
    $trimmed = $Extension.Trim().ToLowerInvariant()
    if (-not $trimmed.StartsWith(".")) {
        $trimmed = ".$trimmed"
    }
    $trimmed
}

$selectedExtensions = @($Extensions | ForEach-Object { Normalize-Extension $_ })
foreach ($extension in $selectedExtensions) {
    if ($supportedExtensions -notcontains $extension) {
        throw "unsupported extension filter: $extension"
    }
}

function Test-SelectedExtension([string]$Extension) {
    if ($supportedExtensions -notcontains $Extension) {
        return $false
    }
    if ($selectedExtensions.Count -eq 0) {
        return $true
    }
    $selectedExtensions -contains $Extension
}

function New-Stats([string]$Label) {
    [ordered]@{
        Label = $Label
        Files = 0
        Archives = 0
        RomEntries = 0
        Headers = 0
        NoHeader = 0
        Copier512 = 0
        HeaderSms = 0
        HeaderGg = 0
        HeaderUnknown = 0
        HintSms = 0
        HintGg = 0
        HintSg = 0
        ResolvedSms = 0
        ResolvedGg = 0
        ResolvedSg = 0
        ReadErrors = 0
    }
}

function Get-HintFromExtension([string]$Extension) {
    switch ($Extension.ToLowerInvariant()) {
        ".sms" { "sms"; break }
        ".gg" { "gg"; break }
        ".sg" { "sg"; break }
        ".sc" { "sg"; break }
        default { $null }
    }
}

function Get-NormalizedRomBytes([byte[]]$Bytes) {
    if ($Bytes.Length -gt $copierHeaderSize -and ($Bytes.Length % $romBankSize) -eq $copierHeaderSize) {
        $normalized = New-Object byte[] ($Bytes.Length - $copierHeaderSize)
        [Array]::Copy($Bytes, $copierHeaderSize, $normalized, 0, $normalized.Length)
        return [PSCustomObject]@{ Bytes = $normalized; CopierHeader = $true }
    }

    [PSCustomObject]@{ Bytes = $Bytes; CopierHeader = $false }
}

function Get-SegaHeader([byte[]]$Bytes) {
    foreach ($offset in $headerOffsets) {
        if ($Bytes.Length -lt ($offset + 16)) {
            continue
        }

        $matches = $true
        for ($i = 0; $i -lt $headerMagic.Length; $i++) {
            if ($Bytes[$offset + $i] -ne $headerMagic[$i]) {
                $matches = $false
                break
            }
        }

        if ($matches) {
            $regionSize = $Bytes[$offset + 0x0F]
            return [PSCustomObject]@{
                Offset = $offset
                Region = ($regionSize -shr 4)
                Size = ($regionSize -band 0x0F)
            }
        }
    }

    $null
}

function Get-SystemFromHeader($Header) {
    if ($null -eq $Header) {
        return $null
    }

    switch ($Header.Region) {
        3 { "sms"; break }
        4 { "sms"; break }
        5 { "gg"; break }
        6 { "gg"; break }
        7 { "gg"; break }
        default { $null }
    }
}

function Add-RomStats($Stats, [byte[]]$Bytes, [string]$Hint) {
    $Stats.RomEntries++

    switch ($Hint) {
        "sms" { $Stats.HintSms++ }
        "gg" { $Stats.HintGg++ }
        "sg" { $Stats.HintSg++ }
    }

    $normalized = Get-NormalizedRomBytes $Bytes
    if ($normalized.CopierHeader) {
        $Stats.Copier512++
    }

    $header = Get-SegaHeader $normalized.Bytes
    $headerSystem = Get-SystemFromHeader $header
    if ($null -eq $header) {
        $Stats.NoHeader++
    } else {
        $Stats.Headers++
        switch ($headerSystem) {
            "sms" { $Stats.HeaderSms++ }
            "gg" { $Stats.HeaderGg++ }
            default { $Stats.HeaderUnknown++ }
        }
    }

    $resolved = if ($null -ne $Hint) { $Hint } elseif ($null -ne $headerSystem) { $headerSystem } else { "sms" }
    switch ($resolved) {
        "sms" { $Stats.ResolvedSms++ }
        "gg" { $Stats.ResolvedGg++ }
        "sg" { $Stats.ResolvedSg++ }
    }
}

function Read-AllBytesFromZipEntry($Entry) {
    $stream = $Entry.Open()
    try {
        $memory = [System.IO.MemoryStream]::new()
        try {
            $stream.CopyTo($memory)
            return $memory.ToArray()
        } finally {
            $memory.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

function Scan-Path([string]$Path, [string]$Label) {
    $stats = New-Stats $Label
    if (-not (Test-Path -LiteralPath $Path)) {
        return [PSCustomObject]$stats
    }

    $pendingDirs = [System.Collections.Generic.Stack[string]]::new()
    $pendingDirs.Push($Path)

    while ($pendingDirs.Count -gt 0) {
        if ($Limit -gt 0 -and $stats.RomEntries -ge $Limit) {
            break
        }

        $dir = $pendingDirs.Pop()
        try {
            foreach ($childDir in [System.IO.Directory]::EnumerateDirectories($dir)) {
                $pendingDirs.Push($childDir)
            }
        } catch {
            $stats.ReadErrors++
            continue
        }

        try {
            $files = [System.IO.Directory]::EnumerateFiles($dir)
        } catch {
            $stats.ReadErrors++
            continue
        }

        foreach ($file in $files) {
            if ($Limit -gt 0 -and $stats.RomEntries -ge $Limit) {
                break
            }

            $extension = [System.IO.Path]::GetExtension($file).ToLowerInvariant()
            if (Test-SelectedExtension $extension) {
                $stats.Files++
                try {
                    Add-RomStats $stats ([System.IO.File]::ReadAllBytes($file)) (Get-HintFromExtension $extension)
                } catch {
                    $stats.ReadErrors++
                }
                continue
            }

            if (-not $SkipArchives -and $extension -eq ".zip") {
                $stats.Archives++
                $archive = $null
                try {
                    $archive = [System.IO.Compression.ZipFile]::OpenRead($file)
                    foreach ($entry in $archive.Entries) {
                        if ($Limit -gt 0 -and $stats.RomEntries -ge $Limit) {
                            break
                        }

                        $entryExtension = [System.IO.Path]::GetExtension($entry.FullName).ToLowerInvariant()
                        if (-not (Test-SelectedExtension $entryExtension)) {
                            continue
                        }

                        try {
                            Add-RomStats $stats (Read-AllBytesFromZipEntry $entry) (Get-HintFromExtension $entryExtension)
                        } catch {
                            $stats.ReadErrors++
                        }
                    }
                } catch {
                    $stats.ReadErrors++
                } finally {
                    if ($null -ne $archive) {
                        $archive.Dispose()
                    }
                }
            }
        }
    }

    [PSCustomObject]$stats
}

if (-not $ProbeOnly) {
    if ($ShowRootPath) {
        Write-Host "Scanning Sega 8-bit ROM metadata under $RomRoot"
    } else {
        Write-Host "Scanning Sega 8-bit ROM metadata under configured root"
    }
    Write-Host "Only aggregate counts are printed; ROM names, paths, and hashes are intentionally omitted."
    if ($selectedExtensions.Count -gt 0) {
        Write-Host ("Extension filter: {0}" -f ($selectedExtensions -join ", "))
    }

    $results = foreach ($dir in $SystemDirs) {
        Scan-Path (Join-Path $RomRoot $dir) $dir
    }

    $results |
        Format-Table -Property Label, Files, Archives, RomEntries, Headers, NoHeader, Copier512, HeaderSms, HeaderGg, HeaderUnknown, HintSms, HintGg, HintSg, ResolvedSms, ResolvedGg, ResolvedSg, ReadErrors -AutoSize |
        Out-String -Width 240 |
        Write-Host
}

if ($Probe) {
    $probeArgs = @(
        "run",
        "-p",
        "zeff-sega8-core",
        "--example",
        "probe_romset",
        "--",
        "--root",
        $RomRoot,
        "--max-instructions",
        $ProbeMaxInstructions
    )

    foreach ($dir in $SystemDirs) {
        $probeArgs += @("--dir", $dir)
    }
    $probeExtensionsToUse = if ($ProbeExtensions.Count -gt 0) { $ProbeExtensions } else { $Extensions }
    foreach ($extension in $probeExtensionsToUse) {
        $probeArgs += @("--extension", $extension)
    }
    if ($Limit -gt 0) {
        $probeArgs += @("--limit", $Limit)
    }
    if ($ProbeFrames -gt 0) {
        $probeArgs += @("--frames", $ProbeFrames)
    }
    if ($SkipArchives) {
        $probeArgs += "--skip-archives"
    }
    if ($ProbeShowPaths) {
        $probeArgs += "--show-paths"
    }
    if (-not [string]::IsNullOrWhiteSpace($IssueReport)) {
        $probeArgs += @("--issue-report", $IssueReport)
    }
    if ($IssueReportPaths) {
        $probeArgs += "--issue-report-paths"
    }
    if ($ShowRootPath) {
        $probeArgs += "--show-root"
    }

    & cargo @probeArgs
}
