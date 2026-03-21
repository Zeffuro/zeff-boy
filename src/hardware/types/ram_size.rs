/// RAM size from header byte (0x149).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RamSize {
    None,   // $00
    Unused, // $01 (not used, historically error)
    Kb8,    // $02 — 8 KiB (1 bank)
    Kb32,   // $03 — 32 KiB (4 banks)
    Kb128,  // $04 — 128 KiB (16 banks)
    Kb64,   // $05 — 64 KiB (8 banks)
    Unknown(u8),
}

impl RamSize {
    pub(crate) fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => RamSize::None,
            0x01 => RamSize::Unused,
            0x02 => RamSize::Kb8,
            0x03 => RamSize::Kb32,
            0x04 => RamSize::Kb128,
            0x05 => RamSize::Kb64,
            unknown => RamSize::Unknown(unknown),
        }
    }

    pub(crate) fn size_bytes(&self) -> usize {
        match self {
            RamSize::None | RamSize::Unused => 0,
            RamSize::Kb8 => 8 * 1024,
            RamSize::Kb32 => 32 * 1024,
            RamSize::Kb128 => 128 * 1024,
            RamSize::Kb64 => 64 * 1024,
            RamSize::Unknown(_) => 0,
        }
    }

    pub(crate) fn banks(&self) -> usize {
        match self {
            RamSize::None | RamSize::Unused => 0,
            RamSize::Kb8 => 1,
            RamSize::Kb32 => 4,
            RamSize::Kb128 => 16,
            RamSize::Kb64 => 8,
            RamSize::Unknown(_) => 0,
        }
    }
}
