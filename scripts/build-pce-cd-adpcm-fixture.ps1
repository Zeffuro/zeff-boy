param(
    [string]$OutDir = "rom-tests/cache/pce/generated/cd-adpcm-irq"
)

$ErrorActionPreference = "Stop"

$systemCardSize = 256 * 1024
$sectorSize = 2048
$sectorCount = 17
$programBase = 0xE000
$cdBase = 0xD800

$program = [System.Collections.Generic.List[byte]]::new()
$labels = @{}
$fixups = [System.Collections.Generic.List[object]]::new()

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

function Add-LdaDirect {
    param([int]$Address)
    Add-Bytes @(0xA5, $Address)
}

function Add-StaDirect {
    param([int]$Address)
    Add-Bytes @(0x85, $Address)
}

function Add-StzDirect {
    param([int]$Address)
    Add-Bytes @(0x64, $Address)
}

function Add-WaitRequest {
    param([string]$Label)
    Mark-Label $Label
    Add-LdaAbsolute ($cdBase + 0)
    Add-Bytes @(0x29, 0x40)
    Add-RelativeLabel 0xF0 $Label
}

function Add-Acknowledge {
    Add-LdaImmediate 0x80
    Add-StaAbsolute ($cdBase + 2)
    Add-StzAbsolute ($cdBase + 2)
}

function Add-FailJump {
    param(
        [int]$Code,
        [string]$ContinueLabel
    )
    Add-RelativeLabel 0xF0 $ContinueLabel
    Add-LdaImmediate $Code
    Add-AbsoluteLabel 0x4C "fail"
    Mark-Label $ContinueLabel
}

Mark-Label "reset"
Add-Bytes @(0x78, 0xD8, 0xD4)
Add-LdaImmediate 0xF8
Add-Bytes @(0x53, 0x03)
Add-LdaImmediate 0xFF
Add-Bytes @(0x53, 0x40)
Add-Bytes @(0xA2, 0xFF, 0x9A)

foreach ($entry in @(
    @(0x00, [char]'Z'),
    @(0x01, [char]'P'),
    @(0x02, [char]'C'),
    @(0x03, [char]'E')
)) {
    Add-LdaImmediate ([int]$entry[1])
    Add-StaDirect $entry[0]
}
foreach ($address in 0x04..0x0E) {
    Add-StzDirect $address
}

Add-LdaImmediate 0x80
Add-StaAbsolute ($cdBase + 13)
Add-StzAbsolute ($cdBase + 8)
Add-StzAbsolute ($cdBase + 9)
Add-LdaImmediate 0x03
Add-StaAbsolute ($cdBase + 13)
Add-StzAbsolute ($cdBase + 0)

$command = @(0x08, 0x00, 0x00, 0x00, $sectorCount, 0x00)
for ($index = 0; $index -lt $command.Count; $index++) {
    Add-WaitRequest ("command_request_{0}" -f $index)
    Add-LdaImmediate $command[$index]
    Add-StaAbsolute ($cdBase + 1)
    Add-Acknowledge
}
Add-LdaImmediate 0x02
Add-StaAbsolute ($cdBase + 11)

Mark-Label "wait_status"
Add-LdaAbsolute ($cdBase + 0)
Add-Bytes @(0x29, 0xD8, 0xC9, 0xD8)
Add-RelativeLabel 0xD0 "wait_status"
Add-LdaAbsolute ($cdBase + 1)
Add-Bytes @(0xC9, 0x00)
Add-FailJump 0x81 "status_ok"
Add-Acknowledge
Add-WaitRequest "message_request"
Add-LdaAbsolute ($cdBase + 1)
Add-Bytes @(0xC9, 0x00)
Add-FailJump 0x82 "message_ok"
Add-Acknowledge

Mark-Label "wait_bus_free"
Add-LdaAbsolute ($cdBase + 0)
Add-RelativeLabel 0x30 "wait_bus_free"

Add-StzAbsolute ($cdBase + 13)
Add-StzAbsolute ($cdBase + 8)
Add-StzAbsolute ($cdBase + 9)
Add-LdaImmediate 0x0C
Add-StaAbsolute ($cdBase + 13)
Add-LdaAbsolute ($cdBase + 10)
Add-Bytes @(0xC9, 0x5A)
Add-FailJump 0x83 "first_byte_ok"

Add-StzAbsolute ($cdBase + 13)
Add-LdaImmediate 0xFF
Add-StaAbsolute ($cdBase + 8)
Add-LdaImmediate 0x87
Add-StaAbsolute ($cdBase + 9)
Add-LdaImmediate 0x0C
Add-StaAbsolute ($cdBase + 13)
Add-LdaAbsolute ($cdBase + 10)
Add-Bytes @(0xC9, 0x49)
Add-FailJump 0x84 "last_byte_ok"

Add-StzAbsolute ($cdBase + 13)
Add-StzAbsolute ($cdBase + 8)
Add-StzAbsolute ($cdBase + 9)
Add-LdaImmediate 0x0C
Add-StaAbsolute ($cdBase + 13)
Add-LdaImmediate 0x0F
Add-StaAbsolute ($cdBase + 14)
Add-LdaImmediate 0x0C
Add-StaAbsolute ($cdBase + 2)
Add-LdaImmediate 0x60
Add-StaAbsolute ($cdBase + 13)
Add-LdaImmediate 0x0E
Add-StaAbsolute ($cdBase + 15)
Add-LdaAbsolute ($cdBase + 15)
Add-Bytes @(0xC9, 0x0E)
Add-FailJump 0x87 "fade_start_ok"
Add-Byte 0x58

Mark-Label "count_loop"
Add-Bytes @(0xE6, 0x06)
Add-RelativeLabel 0xD0 "count_done"
Add-Bytes @(0xE6, 0x07)
Add-RelativeLabel 0xD0 "count_done"
Add-Bytes @(0xE6, 0x08)
Mark-Label "count_done"
Add-LdaDirect 0x05
Add-Bytes @(0xC9, 0x03)
Add-RelativeLabel 0xD0 "count_loop"
Add-Byte 0x78

Add-LdaDirect 0x0B
Add-RelativeLabel 0xF0 "half_high_ok"
Add-LdaImmediate 0x85
Add-AbsoluteLabel 0x4C "fail"
Mark-Label "half_high_ok"
Add-LdaDirect 0x0A
Add-Bytes @(0xC9, 0x20)
Add-RelativeLabel 0xB0 "half_timing_ok"
Add-LdaImmediate 0x85
Add-AbsoluteLabel 0x4C "fail"
Mark-Label "half_timing_ok"

Add-LdaDirect 0x0E
Add-Bytes @(0xC9, 0x08)
Add-RelativeLabel 0xB0 "end_min_ok"
Add-LdaImmediate 0x85
Add-AbsoluteLabel 0x4C "fail"
Mark-Label "end_min_ok"
Add-LdaDirect 0x0E
Add-Bytes @(0xC9, 0x20)
Add-RelativeLabel 0x90 "timing_ok"
Add-LdaImmediate 0x85
Add-AbsoluteLabel 0x4C "fail"
Mark-Label "timing_ok"
Add-LdaAbsolute ($cdBase + 15)
Add-Bytes @(0xC9, 0x0E)
Add-FailJump 0x88 "fade_latch_ok"
Add-LdaImmediate 0x01
Add-StaDirect 0x04
Mark-Label "pass_spin"
Add-RelativeLabel 0x80 "pass_spin"

Mark-Label "fail"
Add-Byte 0x78
Add-StzAbsolute ($cdBase + 2)
Add-StaDirect 0x04
Mark-Label "fail_spin"
Add-RelativeLabel 0x80 "fail_spin"

Mark-Label "irq2"
Add-Byte 0x48
Add-LdaAbsolute ($cdBase + 3)
Add-Bytes @(0x29, 0x04)
Add-RelativeLabel 0xF0 "check_end_irq"
Add-LdaDirect 0x05
Add-RelativeLabel 0xD0 "unexpected_irq"
Add-LdaImmediate 0x01
Add-StaDirect 0x05
for ($index = 0; $index -lt 3; $index++) {
    Add-LdaDirect (0x06 + $index)
    Add-StaDirect (0x09 + $index)
}
Add-LdaImmediate 0x08
Add-StaAbsolute ($cdBase + 2)
Add-Bytes @(0x68, 0x40)

Mark-Label "check_end_irq"
Add-LdaAbsolute ($cdBase + 3)
Add-Bytes @(0x29, 0x08)
Add-RelativeLabel 0xF0 "unexpected_irq"
Add-LdaDirect 0x05
Add-Bytes @(0xC9, 0x01)
Add-RelativeLabel 0xD0 "unexpected_irq"
Add-LdaImmediate 0x03
Add-StaDirect 0x05
for ($index = 0; $index -lt 3; $index++) {
    Add-LdaDirect (0x06 + $index)
    Add-StaDirect (0x0C + $index)
}
Add-StzAbsolute ($cdBase + 2)
Add-Bytes @(0x68, 0x40)

Mark-Label "unexpected_irq"
Add-StzAbsolute ($cdBase + 2)
Add-Byte 0x78
Add-LdaImmediate 0x86
Add-StaDirect 0x04
Mark-Label "irq_fail_spin"
Add-RelativeLabel 0x80 "irq_fail_spin"

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

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$systemCard = [byte[]]::new($systemCardSize)
for ($index = 0; $index -lt $systemCard.Length; $index++) {
    $systemCard[$index] = 0xEA
}
for ($index = 0; $index -lt $program.Count; $index++) {
    $systemCard[$index] = $program[$index]
}
$irq2 = $programBase + [int]$labels["irq2"]
$unexpected = $programBase + [int]$labels["unexpected_irq"]
foreach ($vector in @(
    @(0x1FF6, $irq2),
    @(0x1FF8, $unexpected),
    @(0x1FFA, $unexpected),
    @(0x1FFC, $unexpected),
    @(0x1FFE, $programBase)
)) {
    $systemCard[$vector[0]] = [byte]($vector[1] -band 0xFF)
    $systemCard[$vector[0] + 1] = [byte](($vector[1] -shr 8) -band 0xFF)
}

$data = [byte[]]::new($sectorSize * $sectorCount)
for ($index = 0; $index -lt $data.Length; $index++) {
    $data[$index] = [byte](($index * 17 + 0x5A) -band 0xFF)
}

$systemCardPath = Join-Path $OutDir "syscard3.pce"
$dataPath = Join-Path $OutDir "cd-adpcm-irq.bin"
$cuePath = Join-Path $OutDir "cd-adpcm-irq.cue"
[System.IO.File]::WriteAllBytes($systemCardPath, $systemCard)
[System.IO.File]::WriteAllBytes($dataPath, $data)
$cue = @"
FILE "cd-adpcm-irq.bin" BINARY
  TRACK 01 MODE1/2048
    INDEX 01 00:00:00
"@
[System.IO.File]::WriteAllText($cuePath, $cue.Replace("`r`n", "`n"), [Text.UTF8Encoding]::new($false))

function Get-Sha256Hex {
    param([byte[]]$Bytes)
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return [BitConverter]::ToString($sha256.ComputeHash($Bytes)).Replace("-", "").ToLowerInvariant()
    } finally {
        $sha256.Dispose()
    }
}

$expectedHashes = @{
    $systemCardPath = "4f85f6151a41a5b0244caa7fbb43cac8c67ceb596bcd6d6763028918d09cc81d"
    $dataPath = "8d68a7ed5321eab8e8e28f6f2e80f2b2931f841a5007fa1fa8451f7daef2983a"
    $cuePath = "bb8a1c396d686ee02f52a4122f538a2107f35bca29e26625727093a14584cb4d"
}
foreach ($path in @($systemCardPath, $dataPath, $cuePath)) {
    $item = Get-Item -LiteralPath $path
    $sha = Get-Sha256Hex ([IO.File]::ReadAllBytes($path))
    if ($sha -ne $expectedHashes[$path]) {
        throw "unexpected generated hash for ${path}: $sha"
    }
    Write-Host ("wrote {0} bytes={1} sha256={2}" -f $path, $item.Length, $sha)
}

$identityStream = [IO.MemoryStream]::new()
$identityWriter = [IO.BinaryWriter]::new($identityStream)
foreach ($bytes in @(
    [Text.Encoding]::UTF8.GetBytes("zeff-boy:pce-cd-data:v2"),
    [IO.File]::ReadAllBytes($cuePath)
)) {
    $identityWriter.Write([int64]$bytes.Length)
    $identityWriter.Write($bytes)
}
$identityWriter.Write([int64]1)
foreach ($bytes in @(
    [Text.Encoding]::UTF8.GetBytes("cd-adpcm-irq.bin"),
    [IO.File]::ReadAllBytes($dataPath)
)) {
    $identityWriter.Write([int64]$bytes.Length)
    $identityWriter.Write($bytes)
}
$identityWriter.Flush()
$packageIdentity = Get-Sha256Hex $identityStream.ToArray()
$expectedPackageIdentity = "aa210f18f6f5820a9a3c68d843ed9a817f39b9003b93f587ed7d98ffd4798bd9"
if ($packageIdentity -ne $expectedPackageIdentity) {
    throw "unexpected package content identity: $packageIdentity"
}
Write-Host "package content sha256=$packageIdentity"

$discStream = [IO.MemoryStream]::new()
$discWriter = [IO.BinaryWriter]::new($discStream)
$discWriter.Write([Text.Encoding]::UTF8.GetBytes("zeff-boy:pce-core-cd-disc:v1"))
$discWriter.Write([byte]0)
$discWriter.Write([uint32]1)
$discWriter.Write([byte]1)
$discWriter.Write([byte]4)
$discWriter.Write([byte]0)
$discWriter.Write([uint32]0)
$discWriter.Write([uint32]0)
$discWriter.Write([byte]1)
$discWriter.Write([int64]$data.Length)
$discWriter.Write($data)
$discWriter.Flush()
$discIdentity = Get-Sha256Hex $discStream.ToArray()
$expectedDiscIdentity = "c8c7426b3f91d7bfb5f5029ffe18d8e2604195daf8f54c8b2494c2981e8f68a2"
if ($discIdentity -ne $expectedDiscIdentity) {
    throw "unexpected normalized disc identity: $discIdentity"
}
Write-Host "normalized disc sha256=$discIdentity"
Write-Host ("program bytes={0} irq2={1:X4}" -f $program.Count, $irq2)
