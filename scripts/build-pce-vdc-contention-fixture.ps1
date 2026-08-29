param(
    [string]$OutDir = "rom-tests/cache/pce/generated/vdc-fetch-contention"
)

$ErrorActionPreference = "Stop"

$romSize = 8 * 1024
$programBase = 0xE000
$statusBase = 0x2000
$rasterBase = 0x60
$rowAddress = 0x2015
$frameAddress = 0x2016
$sourceAddress = 0x2100
$vdcDataLow = 0x0002
$vdcDataHigh = 0x0003
$vceAddressLow = 0x0402
$vceAddressHigh = 0x0403
$vceDataLow = 0x0404
$vceDataHigh = 0x0405

$program = [Collections.Generic.List[byte]]::new()
$labels = @{}
$fixups = [Collections.Generic.List[object]]::new()

function Add-Byte {
    param([int]$Value)
    [void]$program.Add([byte]($Value -band 0xFF))
}

function Add-Bytes {
    param([int[]]$Values)
    foreach ($value in $Values) {
        Add-Byte $value
    }
}

function Mark-Label {
    param([string]$Name)
    if ($labels.ContainsKey($Name)) {
        throw "duplicate label: $Name"
    }
    $labels[$Name] = $program.Count
}

function Add-AbsoluteLabel {
    param(
        [int]$Opcode,
        [string]$Label
    )
    Add-Byte $Opcode
    $fixups.Add([pscustomobject]@{
        Kind = "absolute"
        Offset = $program.Count
        Label = $Label
    })
    Add-Bytes @(0, 0)
}

function Add-RelativeLabel {
    param(
        [int]$Opcode,
        [string]$Label
    )
    Add-Byte $Opcode
    $fixups.Add([pscustomobject]@{
        Kind = "relative"
        Offset = $program.Count
        Label = $Label
    })
    Add-Byte 0
}

function Add-LdaImmediate {
    param([int]$Value)
    Add-Bytes @(0xA9, $Value)
}

function Add-LdaAbsolute {
    param([int]$Address)
    Add-Bytes @(0xAD, ($Address -band 0xFF), (($Address -shr 8) -band 0xFF))
}

function Add-StaAbsolute {
    param([int]$Address)
    Add-Bytes @(0x8D, ($Address -band 0xFF), (($Address -shr 8) -band 0xFF))
}

function Add-StzAbsolute {
    param([int]$Address)
    Add-Bytes @(0x9C, ($Address -band 0xFF), (($Address -shr 8) -band 0xFF))
}

function Add-VdcRegister {
    param(
        [int]$Register,
        [int]$Value
    )
    Add-Bytes @(0x03, $Register, 0x13, ($Value -band 0xFF), 0x23, (($Value -shr 8) -band 0xFF))
}

Mark-Label "reset"
Add-Bytes @(0x78, 0xD8, 0xD4, 0xA2, 0xFF, 0x9A)
Add-LdaImmediate 0xFF
Add-Bytes @(0x53, 0x01)
Add-LdaImmediate 0xF8
Add-Bytes @(0x53, 0x02)

$status = @(
    [char]'Z', [char]'P', [char]'C', [char]'E',
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    [char]'V', [char]'D', [char]'C', [char]'S',
    1, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF
)
for ($index = 0; $index -lt $status.Count; $index++) {
    Add-LdaImmediate ([int]$status[$index])
    Add-StaAbsolute ($statusBase + $index)
}

for ($index = 0; $index -lt 16; $index++) {
    Add-LdaImmediate (($index * 0x11) -band 0xFF)
    Add-StaAbsolute ($sourceAddress + $index)
}

Add-StzAbsolute 0x0400
foreach ($address in @($vceAddressLow, $vceAddressHigh, $vceDataLow, $vceDataHigh)) {
    Add-StzAbsolute $address
}

Add-VdcRegister 0x05 0x0000
Add-VdcRegister 0x07 0x0000
Add-VdcRegister 0x08 0x0000
Add-VdcRegister 0x09 0x0000
Add-VdcRegister 0x0A 0x0202
Add-VdcRegister 0x0B 0x041F
Add-VdcRegister 0x0C 0x1702
Add-VdcRegister 0x0D 0x00DF
Add-VdcRegister 0x0E 0x000A
Add-VdcRegister 0x06 0x0000
Add-VdcRegister 0x05 0x008C
Add-StzAbsolute 0x1402
Add-StzAbsolute 0x1403
Add-Byte 0x58
Mark-Label "main_spin"
Add-RelativeLabel 0x80 "main_spin"

Mark-Label "irq1"
Add-LdaAbsolute 0x0000
Add-Bytes @(0x29, 0x20)
Add-RelativeLabel 0xF0 "raster"
Add-AbsoluteLabel 0x4C "vblank"

Mark-Label "raster"

Add-LdaAbsolute $rowAddress
for ($index = 0; $index -lt 4; $index++) {
    Add-Byte 0x4A
}
Add-Bytes @(0x29, 0x03, 0xAA)
Add-Byte 0xBD
$fixups.Add([pscustomobject]@{
    Kind = "absolute"
    Offset = $program.Count
    Label = "mwr_modes"
})
Add-Bytes @(0, 0)
Add-Bytes @(0x03, 0x09)
Add-StaAbsolute $vdcDataLow
Add-StzAbsolute $vdcDataHigh
Add-Bytes @(0x03, 0x00)
Add-StzAbsolute $vdcDataLow
Add-LdaImmediate 0x30
Add-StaAbsolute $vdcDataHigh
Add-Bytes @(0x03, 0x02)

foreach ($address in @($vceAddressLow, $vceAddressHigh, $vceDataLow, $vceDataHigh)) {
    Add-StzAbsolute $address
}
Add-Bytes @(
    0xE3,
    ($sourceAddress -band 0xFF), (($sourceAddress -shr 8) -band 0xFF),
    ($vdcDataLow -band 0xFF), (($vdcDataLow -shr 8) -band 0xFF),
    0x10, 0x00
)

Add-LdaAbsolute $rowAddress
for ($index = 0; $index -lt 4; $index++) {
    Add-Byte 0x4A
}
Add-Bytes @(0x29, 0x03, 0xAA, 0xBD)
$fixups.Add([pscustomobject]@{
    Kind = "absolute"
    Offset = $program.Count
    Label = "color_low"
})
Add-Bytes @(0, 0)
Add-StzAbsolute $vceAddressLow
Add-StzAbsolute $vceAddressHigh
Add-StaAbsolute $vceDataLow
Add-Byte 0xBD
$fixups.Add([pscustomobject]@{
    Kind = "absolute"
    Offset = $program.Count
    Label = "color_high"
})
Add-Bytes @(0, 0)
Add-StaAbsolute $vceDataHigh

Add-Bytes @(0xEE, ($rowAddress -band 0xFF), (($rowAddress -shr 8) -band 0xFF))
Add-LdaAbsolute $rowAddress
Add-Bytes @(0xC9, 0x40)
Add-RelativeLabel 0x90 "schedule_next"

Add-LdaImmediate 0x01
Add-StaAbsolute ($statusBase + 4)
Add-LdaImmediate 0x0F
Add-StaAbsolute ($statusBase + 5)
foreach ($address in @(($statusBase + 6), ($statusBase + 9), ($statusBase + 12))) {
    Add-LdaImmediate 0x10
    Add-StaAbsolute $address
}
Add-LdaImmediate 0x0F
Add-StaAbsolute ($statusBase + 20)
Add-Bytes @(0x03, 0x06)
Add-StzAbsolute $vdcDataLow
Add-StzAbsolute $vdcDataHigh
Add-Byte 0x40

Mark-Label "schedule_next"
Add-Bytes @(0x0A, 0x18, 0x69, $rasterBase, 0x03, 0x06)
Add-StaAbsolute $vdcDataLow
Add-StzAbsolute $vdcDataHigh
Add-Byte 0x40

Mark-Label "vblank"
Add-StzAbsolute $rowAddress
Add-Bytes @(0xEE, ($frameAddress -band 0xFF), (($frameAddress -shr 8) -band 0xFF))
Add-Bytes @(0x03, 0x06)
Add-LdaImmediate $rasterBase
Add-StaAbsolute $vdcDataLow
Add-StzAbsolute $vdcDataHigh
Add-Byte 0x40

Mark-Label "unexpected"
Add-Byte 0x40

Mark-Label "mwr_modes"
Add-Bytes @(0x00, 0x01, 0x02, 0x03)
Mark-Label "color_low"
Add-Bytes @(0x38, 0x07, 0xC0, 0xFF)
Mark-Label "color_high"
Add-Bytes @(0x00, 0x00, 0x01, 0x01)

foreach ($fixup in $fixups) {
    if (-not $labels.ContainsKey($fixup.Label)) {
        throw "undefined label: $($fixup.Label)"
    }
    $target = [int]$labels[$fixup.Label]
    if ($fixup.Kind -eq "absolute") {
        $address = $programBase + $target
        $program[$fixup.Offset] = [byte]($address -band 0xFF)
        $program[$fixup.Offset + 1] = [byte](($address -shr 8) -band 0xFF)
    } else {
        $delta = $target - ($fixup.Offset + 1)
        if ($delta -lt -128 -or $delta -gt 127) {
            throw "relative branch to $($fixup.Label) is out of range: $delta"
        }
        $program[$fixup.Offset] = [byte]($delta -band 0xFF)
    }
}

if ($program.Count -gt 0x1FF6) {
    throw "fixture program overlaps vectors"
}

$rom = [byte[]]::new($romSize)
for ($index = 0; $index -lt $rom.Length; $index++) {
    $rom[$index] = 0xEA
}
for ($index = 0; $index -lt $program.Count; $index++) {
    $rom[$index] = $program[$index]
}

$irq1 = $programBase + [int]$labels["irq1"]
$unexpected = $programBase + [int]$labels["unexpected"]
foreach ($vector in @(
    @(0x1FF6, $unexpected),
    @(0x1FF8, $irq1),
    @(0x1FFA, $unexpected),
    @(0x1FFC, $unexpected),
    @(0x1FFE, $programBase)
)) {
    $rom[$vector[0]] = [byte]($vector[1] -band 0xFF)
    $rom[$vector[0] + 1] = [byte](($vector[1] -shr 8) -band 0xFF)
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$romPath = Join-Path $OutDir "vdc-fetch-contention.pce"
[IO.File]::WriteAllBytes($romPath, $rom)

$sha256 = [Security.Cryptography.SHA256]::Create()
try {
    $hash = [BitConverter]::ToString($sha256.ComputeHash($rom)).Replace("-", "").ToLowerInvariant()
} finally {
    $sha256.Dispose()
}

$expectedHash = "7edae12b94a85d7cf740d7fd86ce2a770051edc693efdda8504ed76f407c055d"
if ($hash -ne $expectedHash) {
    throw "unexpected generated hash for ${romPath}: $hash"
}
Write-Host ("wrote {0} bytes={1} sha256={2} program={3}" -f $romPath, $rom.Length, $hash, $program.Count)
