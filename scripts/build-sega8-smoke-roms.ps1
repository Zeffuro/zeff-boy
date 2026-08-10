param(
    [string]$OutDir = "rom-tests/cache/sega8/generated"
)

$ErrorActionPreference = "Stop"

$romBankSize = 0x4000
$codemastersHeaderOffset = 0x7FE0
$codemastersHeaderSize = 16
$codemastersHeaderBankCount = 0x00
$codemastersHeaderDay = 0x01
$codemastersHeaderMonth = 0x02
$codemastersHeaderYear = 0x03
$codemastersHeaderHour = 0x04
$codemastersHeaderMinute = 0x05
$codemastersHeaderChecksum = 0x06
$codemastersHeaderComplement = 0x08
$codemastersHeaderZeroPaddingStart = 0x0A

function New-ByteList {
    $bytes = [System.Collections.Generic.List[byte]]::new()
    return ,$bytes
}

function Add-Byte {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Value
    )
    [void]$Bytes.Add([byte]($Value -band 0xFF))
}

function Add-Bytes {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int[]]$Values
    )
    foreach ($value in $Values) {
        Add-Byte $Bytes $value
    }
}

function Add-LdA {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Value
    )
    Add-Bytes $Bytes @(0x3E, $Value)
}

function Add-OutA {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Port,
        [int]$Value
    )
    Add-LdA $Bytes $Value
    Add-Bytes $Bytes @(0xD3, $Port)
}

function Add-VdpRegisterWrite {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Register,
        [int]$Value
    )
    Add-OutA $Bytes 0xBF $Value
    Add-OutA $Bytes 0xBF (0x80 -bor $Register)
}

function Add-VdpAddress {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Address,
        [bool]$Cram
    )
    Add-OutA $Bytes 0xBF ($Address -band 0xFF)
    $high = (($Address -shr 8) -band 0x3F)
    if ($Cram) {
        $high = $high -bor 0xC0
    } else {
        $high = $high -bor 0x40
    }
    Add-OutA $Bytes 0xBF $high
}

function Add-VramWrite {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Address,
        [int[]]$Values
    )
    Add-VdpAddress $Bytes $Address $false
    foreach ($value in $Values) {
        Add-OutA $Bytes 0xBE $value
    }
}

function Add-CramWrite {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Address,
        [int[]]$Values
    )
    Add-VdpAddress $Bytes $Address $true
    foreach ($value in $Values) {
        Add-OutA $Bytes 0xBE $value
    }
}

function Add-ProgramPrefix {
    param([System.Collections.Generic.List[byte]]$Bytes)
    Add-Bytes $Bytes @(0xF3, 0x31, 0xF0, 0xDF)
}

function Add-HaltLoop {
    param([System.Collections.Generic.List[byte]]$Bytes)
    Add-Bytes $Bytes @(0x76, 0x18, 0xFD)
}

function Add-LdAbsoluteA {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Address
    )
    Add-Bytes $Bytes @(0x32, ($Address -band 0xFF), (($Address -shr 8) -band 0xFF))
}

function Add-Jp {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$Address
    )
    Add-Bytes $Bytes @(0xC3, ($Address -band 0xFF), (($Address -shr 8) -band 0xFF))
}

function Add-Mode4Setup {
    param([System.Collections.Generic.List[byte]]$Bytes)
    Add-VdpRegisterWrite $Bytes 0 0x04
    Add-VdpRegisterWrite $Bytes 1 0x40
    Add-VdpRegisterWrite $Bytes 2 0x0E
    Add-VdpRegisterWrite $Bytes 5 0x7E
    Add-VdpRegisterWrite $Bytes 7 0x00
}

function New-FilledMode4Tile {
    param([int]$Color)
    $rows = @()
    $plane0 = if (($Color -band 0x01) -ne 0) { 0xFF } else { 0x00 }
    $plane1 = if (($Color -band 0x02) -ne 0) { 0xFF } else { 0x00 }
    $plane2 = if (($Color -band 0x04) -ne 0) { 0xFF } else { 0x00 }
    $plane3 = if (($Color -band 0x08) -ne 0) { 0xFF } else { 0x00 }
    for ($row = 0; $row -lt 8; $row++) {
        $rows += @($plane0, $plane1, $plane2, $plane3)
    }
    return [int[]]$rows
}

function Add-Mode4NameEntry {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$TileX,
        [int]$TileY,
        [int]$Entry
    )
    $offset = 0x3800 + (($TileY * 32 + $TileX) * 2)
    Add-VramWrite $Bytes $offset @((($Entry -band 0xFF)), ((($Entry -shr 8) -band 0xFF)))
}

function Add-Mode4Sprite {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [int]$X,
        [int]$Y,
        [int]$Tile
    )
    Add-VramWrite $Bytes 0x3F00 @(((($Y - 1) -band 0xFF)), 0xD0)
    Add-VramWrite $Bytes 0x3F80 @($X, $Tile)
}

function ConvertTo-PaddedRom {
    param([System.Collections.Generic.List[byte]]$Bytes)
    $length = [Math]::Max(0x4000, $Bytes.Count)
    $rom = [byte[]]::new($length)
    for ($i = 0; $i -lt $Bytes.Count; $i++) {
        $rom[$i] = $Bytes[$i]
    }
    return $rom
}

function Write-Rom {
    param(
        [string]$Path,
        [System.Collections.Generic.List[byte]]$Bytes
    )
    $rom = ConvertTo-PaddedRom $Bytes
    [System.IO.File]::WriteAllBytes($Path, $rom)
    $sha = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    Write-Host ("wrote {0} bytes={1} sha256={2}" -f $Path, $rom.Length, $sha)
}

function Write-RomBytes {
    param(
        [string]$Path,
        [byte[]]$Rom
    )
    [System.IO.File]::WriteAllBytes($Path, $Rom)
    $sha = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    Write-Host ("wrote {0} bytes={1} sha256={2}" -f $Path, $Rom.Length, $sha)
}

function Copy-ProgramToRom {
    param(
        [byte[]]$Rom,
        [int]$Offset,
        [System.Collections.Generic.List[byte]]$Program
    )
    for ($i = 0; $i -lt $Program.Count; $i++) {
        $Rom[$Offset + $i] = $Program[$i]
    }
}

function Add-CodemastersHeader {
    param(
        [byte[]]$Rom,
        [int]$BankCount
    )
    $offset = $codemastersHeaderOffset
    if ($Rom.Length -lt ($offset + $codemastersHeaderSize)) {
        throw "Codemasters header requires at least 32 KiB"
    }

    $Rom[$offset + $codemastersHeaderBankCount] = [byte]$BankCount
    $Rom[$offset + $codemastersHeaderDay] = 0x31
    $Rom[$offset + $codemastersHeaderMonth] = 0x08
    $Rom[$offset + $codemastersHeaderYear] = 0x93
    $Rom[$offset + $codemastersHeaderHour] = 0x10
    $Rom[$offset + $codemastersHeaderMinute] = 0x59
    [Array]::Copy([BitConverter]::GetBytes([uint16]0x1234), 0, $Rom, $offset + $codemastersHeaderChecksum, 2)
    [Array]::Copy([BitConverter]::GetBytes([uint16]0xEDCC), 0, $Rom, $offset + $codemastersHeaderComplement, 2)
    for ($i = $codemastersHeaderZeroPaddingStart; $i -lt $codemastersHeaderSize; $i++) {
        $Rom[$offset + $i] = 0
    }
}

function New-SmsPriorityRom {
    $bytes = New-ByteList
    Add-ProgramPrefix $bytes
    Add-Mode4Setup $bytes
    $cram = @(0x00, 0x03)
    for ($i = 2; $i -lt 17; $i++) {
        $cram += 0x00
    }
    $cram += 0x30
    Add-CramWrite $bytes 0 $cram
    Add-VramWrite $bytes 0x0020 (New-FilledMode4Tile 1)
    Add-VramWrite $bytes 0x0040 (New-FilledMode4Tile 1)
    Add-Mode4NameEntry $bytes 2 2 (0x1000 -bor 1)
    Add-Mode4Sprite $bytes 16 16 2
    Add-HaltLoop $bytes
    return $bytes
}

function New-GgPriorityRom {
    $bytes = New-ByteList
    Add-ProgramPrefix $bytes
    Add-Mode4Setup $bytes
    $cram = @()
    for ($i = 0; $i -lt 36; $i++) {
        $cram += 0x00
    }
    $cram[2] = 0x0F
    $cram[3] = 0x00
    $cram[34] = 0x00
    $cram[35] = 0x0F
    Add-CramWrite $bytes 0 $cram
    Add-VramWrite $bytes 0x0020 (New-FilledMode4Tile 1)
    Add-VramWrite $bytes 0x0040 (New-FilledMode4Tile 1)
    Add-Mode4NameEntry $bytes 7 4 (0x1000 -bor 1)
    Add-Mode4Sprite $bytes 56 32 2
    Add-HaltLoop $bytes
    return $bytes
}

function New-SgTmsRom {
    $bytes = New-ByteList
    Add-ProgramPrefix $bytes
    Add-VdpRegisterWrite $bytes 1 0x40
    Add-VdpRegisterWrite $bytes 2 0x0E
    Add-VdpRegisterWrite $bytes 3 0x20
    Add-VdpRegisterWrite $bytes 4 0x00
    Add-VdpRegisterWrite $bytes 5 0x7F
    Add-VdpRegisterWrite $bytes 6 0x00
    Add-VdpRegisterWrite $bytes 7 0x01
    $pattern = @()
    for ($row = 0; $row -lt 8; $row++) {
        $pattern += 0xFF
    }
    Add-VramWrite $bytes 0x0008 $pattern
    Add-VramWrite $bytes 0x0800 @(0x60)
    Add-VramWrite $bytes (0x3800 + ((2 * 32 + 2))) @(1)
    Add-VramWrite $bytes 0x3F80 @(0xD0)
    Add-HaltLoop $bytes
    return $bytes
}

function New-CodemastersMapperRom {
    $bankCount = 4
    $rom = [byte[]]::new($bankCount * $romBankSize)

    $boot = New-ByteList
    Add-ProgramPrefix $boot
    Add-LdA $boot 3
    Add-LdAbsoluteA $boot 0x8000
    Add-Jp $boot 0x8000
    Copy-ProgramToRom $rom 0 $boot

    $mappedProgram = New-SmsPriorityRom
    Copy-ProgramToRom $rom (3 * $romBankSize) $mappedProgram
    Add-CodemastersHeader $rom $bankCount
    return $rom
}

if ([System.IO.Path]::IsPathRooted($OutDir)) {
    $resolvedOutDir = $OutDir
} else {
    $resolvedOutDir = Join-Path (Get-Location).Path $OutDir
}
New-Item -ItemType Directory -Force -Path $resolvedOutDir | Out-Null

Write-Rom (Join-Path $resolvedOutDir "sms-mode4-priority.sms") (New-SmsPriorityRom)
Write-Rom (Join-Path $resolvedOutDir "gg-mode4-priority.gg") (New-GgPriorityRom)
Write-Rom (Join-Path $resolvedOutDir "sg-tms-graphics.sg") (New-SgTmsRom)
Write-RomBytes (Join-Path $resolvedOutDir "sms-codemasters-mapper.sms") (New-CodemastersMapperRom)
