#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CartridgeType {
    RomOnly,                    // $00
    Mbc1,                       // $01
    Mbc1Ram,                    // $02
    Mbc1RamBattery,             // $03
    Mbc2,                       // $05
    Mbc2Battery,                // $06
    RomRam,                     // $08
    RomRamBattery,              // $09
    Mmm01,                      // $0B
    Mmm01Ram,                   // $0C
    Mmm01RamBattery,            // $0D
    Mbc3TimerBattery,           // $0F
    Mbc3TimerRamBattery,        // $10
    Mbc3,                       // $11
    Mbc3Ram,                    // $12
    Mbc3RamBattery,             // $13
    Mbc5,                       // $19
    Mbc5Ram,                    // $1A
    Mbc5RamBattery,             // $1B
    Mbc5Rumble,                 // $1C
    Mbc5RumbleRam,              // $1D
    Mbc5RumbleRamBattery,       // $1E
    Mbc6,                       // $20
    Mbc7SensorRumbleRamBattery, // $22
    PocketCamera,               // $FC
    BandaiTama5,                // $FD
    HuC3,                       // $FE
    HuC1RamBattery,             // $FF
    Unknown(u8),                // Unknown
}

impl CartridgeType {
    pub(crate) fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => CartridgeType::RomOnly,
            0x01 => CartridgeType::Mbc1,
            0x02 => CartridgeType::Mbc1Ram,
            0x03 => CartridgeType::Mbc1RamBattery,
            0x05 => CartridgeType::Mbc2,
            0x06 => CartridgeType::Mbc2Battery,
            0x08 => CartridgeType::RomRam,
            0x09 => CartridgeType::RomRamBattery,
            0x0B => CartridgeType::Mmm01,
            0x0C => CartridgeType::Mmm01Ram,
            0x0D => CartridgeType::Mmm01RamBattery,
            0x0F => CartridgeType::Mbc3TimerBattery,
            0x10 => CartridgeType::Mbc3TimerRamBattery,
            0x11 => CartridgeType::Mbc3,
            0x12 => CartridgeType::Mbc3Ram,
            0x13 => CartridgeType::Mbc3RamBattery,
            0x19 => CartridgeType::Mbc5,
            0x1A => CartridgeType::Mbc5Ram,
            0x1B => CartridgeType::Mbc5RamBattery,
            0x1C => CartridgeType::Mbc5Rumble,
            0x1D => CartridgeType::Mbc5RumbleRam,
            0x1E => CartridgeType::Mbc5RumbleRamBattery,
            0x20 => CartridgeType::Mbc6,
            0x22 => CartridgeType::Mbc7SensorRumbleRamBattery,
            0xFC => CartridgeType::PocketCamera,
            0xFD => CartridgeType::BandaiTama5,
            0xFE => CartridgeType::HuC3,
            0xFF => CartridgeType::HuC1RamBattery,
            unknown => CartridgeType::Unknown(unknown),
        }
    }

    pub(crate) fn is_battery_backed(self) -> bool {
        matches!(
            self,
            CartridgeType::RomRamBattery
                | CartridgeType::Mbc1RamBattery
                | CartridgeType::Mbc2Battery
                | CartridgeType::Mbc3TimerBattery
                | CartridgeType::Mbc3TimerRamBattery
                | CartridgeType::Mbc3RamBattery
                | CartridgeType::Mbc5RamBattery
                | CartridgeType::Mbc5RumbleRamBattery
                | CartridgeType::Mmm01RamBattery
                | CartridgeType::Mbc7SensorRumbleRamBattery
                | CartridgeType::HuC1RamBattery
        )
    }
}
