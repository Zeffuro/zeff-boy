#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RomSize {
    Kb32,     // $00
    Kb64,     // $01
    Kb128,    // $02
    Kb256,    // $03
    Kb512,    // $04
    Mb1,      // $05
    Mb2,      // $06
    Mb4,      // $07
    Mb8,      // $08
    Kb1100,   // $52 (1.1 MiB/72 banks)
    Kb1200,   // $53 (1.2 MiB/80 banks)
    Kb1500,   // $54 (1.5 MiB/96 banks)
    Unknown(u8),
}

impl RomSize {
    pub(crate) fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => RomSize::Kb32,
            0x01 => RomSize::Kb64,
            0x02 => RomSize::Kb128,
            0x03 => RomSize::Kb256,
            0x04 => RomSize::Kb512,
            0x05 => RomSize::Mb1,
            0x06 => RomSize::Mb2,
            0x07 => RomSize::Mb4,
            0x08 => RomSize::Mb8,
            0x52 => RomSize::Kb1100,
            0x53 => RomSize::Kb1200,
            0x54 => RomSize::Kb1500,
            unknown => RomSize::Unknown(unknown),
        }
    }

    pub(crate) fn size_bytes(&self) -> usize {
        match self {
            RomSize::Kb32 => 32 * 1024,
            RomSize::Kb64 => 64 * 1024,
            RomSize::Kb128 => 128 * 1024,
            RomSize::Kb256 => 256 * 1024,
            RomSize::Kb512 => 512 * 1024,
            RomSize::Mb1 => 1024 * 1024,
            RomSize::Mb2 => 2 * 1024 * 1024,
            RomSize::Mb4 => 4 * 1024 * 1024,
            RomSize::Mb8 => 8 * 1024 * 1024,
            RomSize::Kb1100 => 1152 * 1024, // 1.1MiB = 1,152KiB (approx)
            RomSize::Kb1200 => 1229 * 1024, // 1.2MiB = 1,229KiB (approx)
            RomSize::Kb1500 => 1536 * 1024, // 1.5MiB = 1,536KiB (approx)
            RomSize::Unknown(_) => 0,
        }
    }

    pub(crate) fn banks(&self) -> usize {
        match self {
            RomSize::Kb32 => 2,
            RomSize::Kb64 => 4,
            RomSize::Kb128 => 8,
            RomSize::Kb256 => 16,
            RomSize::Kb512 => 32,
            RomSize::Mb1 => 64,
            RomSize::Mb2 => 128,
            RomSize::Mb4 => 256,
            RomSize::Mb8 => 512,
            RomSize::Kb1100 => 72,
            RomSize::Kb1200 => 80,
            RomSize::Kb1500 => 96,
            RomSize::Unknown(_) => 0,
        }
    }
}