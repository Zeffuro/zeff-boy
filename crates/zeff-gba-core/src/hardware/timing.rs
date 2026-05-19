#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessType {
    NonSequential,
    Sequential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusRegion {
    Bios,
    Ewram,
    Iwram,
    Io,
    PaletteRam,
    Vram,
    Oam,
    GamePak0,
    GamePak1,
    GamePak2,
    Sram,
    Unused,
}

pub fn region_for_addr(addr: u32) -> BusRegion {
    match addr {
        0x0000_0000..=0x0000_3FFF => BusRegion::Bios,
        0x0200_0000..=0x02FF_FFFF => BusRegion::Ewram,
        0x0300_0000..=0x03FF_FFFF => BusRegion::Iwram,
        0x0400_0000..=0x0400_03FF => BusRegion::Io,
        0x0500_0000..=0x05FF_FFFF => BusRegion::PaletteRam,
        0x0600_0000..=0x06FF_FFFF => BusRegion::Vram,
        0x0700_0000..=0x07FF_FFFF => BusRegion::Oam,
        0x0800_0000..=0x09FF_FFFF => BusRegion::GamePak0,
        0x0A00_0000..=0x0BFF_FFFF => BusRegion::GamePak1,
        0x0C00_0000..=0x0DFF_FFFF => BusRegion::GamePak2,
        0x0E00_0000..=0x0E00_FFFF => BusRegion::Sram,
        _ => BusRegion::Unused,
    }
}

pub fn access_cycles(addr: u32, width_bytes: u8, access: AccessType) -> u32 {
    let base = match (region_for_addr(addr), access) {
        (BusRegion::Bios, _) => 1,
        (BusRegion::Ewram, _) => 3,
        (BusRegion::Iwram, _) => 1,
        (BusRegion::Io, _) => 1,
        (BusRegion::PaletteRam | BusRegion::Vram | BusRegion::Oam, _) => 1,
        (BusRegion::GamePak0, AccessType::NonSequential) => 5,
        (BusRegion::GamePak0, AccessType::Sequential) => 3,
        (BusRegion::GamePak1, AccessType::NonSequential) => 5,
        (BusRegion::GamePak1, AccessType::Sequential) => 3,
        (BusRegion::GamePak2, AccessType::NonSequential) => 9,
        (BusRegion::GamePak2, AccessType::Sequential) => 3,
        (BusRegion::Sram, _) => 5,
        (BusRegion::Unused, _) => 1,
    };

    if width_bytes >= 4 {
        match region_for_addr(addr) {
            BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2 => {
                base + sequential_cycles(addr + 2)
            }
            _ => base,
        }
    } else {
        base
    }
}

pub fn instruction_fetch_cycles(addr: u32, width_bytes: u8, sequential: bool) -> u32 {
    access_cycles(
        addr,
        width_bytes,
        if sequential {
            AccessType::Sequential
        } else {
            AccessType::NonSequential
        },
    )
}

fn sequential_cycles(addr: u32) -> u32 {
    access_cycles(addr, 2, AccessType::Sequential)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_regions() {
        assert_eq!(region_for_addr(0x0200_0000), BusRegion::Ewram);
        assert_eq!(region_for_addr(0x0300_0000), BusRegion::Iwram);
        assert_eq!(region_for_addr(0x0800_0000), BusRegion::GamePak0);
        assert_eq!(region_for_addr(0x0E00_0000), BusRegion::Sram);
    }

    #[test]
    fn gamepak_sequential_fetch_is_faster_than_nonsequential() {
        assert!(
            instruction_fetch_cycles(0x0800_0000, 2, true)
                < instruction_fetch_cycles(0x0800_0000, 2, false)
        );
    }
}
