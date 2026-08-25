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

const BIOS_START: u32 = 0x0000_0000;
const BIOS_END: u32 = 0x0000_3FFF;
const EWRAM_START: u32 = 0x0200_0000;
const EWRAM_END: u32 = 0x02FF_FFFF;
const IWRAM_START: u32 = 0x0300_0000;
const IWRAM_END: u32 = 0x03FF_FFFF;
const IO_START: u32 = 0x0400_0000;
const IO_END: u32 = 0x0400_03FF;
const PALETTE_RAM_START: u32 = 0x0500_0000;
const PALETTE_RAM_END: u32 = 0x05FF_FFFF;
const VRAM_START: u32 = 0x0600_0000;
const VRAM_END: u32 = 0x06FF_FFFF;
const OAM_START: u32 = 0x0700_0000;
const OAM_END: u32 = 0x07FF_FFFF;
const GAMEPAK0_START: u32 = 0x0800_0000;
const GAMEPAK0_END: u32 = 0x09FF_FFFF;
const GAMEPAK1_START: u32 = 0x0A00_0000;
const GAMEPAK1_END: u32 = 0x0BFF_FFFF;
const GAMEPAK2_START: u32 = 0x0C00_0000;
const GAMEPAK2_END: u32 = 0x0DFF_FFFF;
const SRAM_START: u32 = 0x0E00_0000;
const SRAM_END: u32 = 0x0E00_FFFF;
const HALFWORD_BYTES: u32 = 2;
const WORD_BYTES: u8 = 4;
const WAITCNT_MASK_2BIT: u16 = 0x03;
const WAITCNT_MASK_1BIT: u16 = 0x01;
const WAITCNT_SRAM_SHIFT: u16 = 0;
const WAITCNT_GAMEPAK0_FIRST_SHIFT: u16 = 2;
const WAITCNT_GAMEPAK0_SECOND_SHIFT: u16 = 4;
const WAITCNT_GAMEPAK1_FIRST_SHIFT: u16 = 5;
const WAITCNT_GAMEPAK1_SECOND_SHIFT: u16 = 7;
const WAITCNT_GAMEPAK2_FIRST_SHIFT: u16 = 8;
const WAITCNT_GAMEPAK2_SECOND_SHIFT: u16 = 10;
const GAMEPAK0_SECOND_CYCLES: [u32; 2] = [3, 2];
const GAMEPAK1_SECOND_CYCLES: [u32; 2] = [5, 2];
const GAMEPAK2_SECOND_CYCLES: [u32; 2] = [9, 2];
const ACCESS_CYCLE_TABLE: [u32; 4] = [5, 4, 3, 9];

pub fn region_for_addr(addr: u32) -> BusRegion {
    match addr {
        BIOS_START..=BIOS_END => BusRegion::Bios,
        EWRAM_START..=EWRAM_END => BusRegion::Ewram,
        IWRAM_START..=IWRAM_END => BusRegion::Iwram,
        IO_START..=IO_END => BusRegion::Io,
        PALETTE_RAM_START..=PALETTE_RAM_END => BusRegion::PaletteRam,
        VRAM_START..=VRAM_END => BusRegion::Vram,
        OAM_START..=OAM_END => BusRegion::Oam,
        GAMEPAK0_START..=GAMEPAK0_END => BusRegion::GamePak0,
        GAMEPAK1_START..=GAMEPAK1_END => BusRegion::GamePak1,
        GAMEPAK2_START..=GAMEPAK2_END => BusRegion::GamePak2,
        SRAM_START..=SRAM_END => BusRegion::Sram,
        _ => BusRegion::Unused,
    }
}

pub fn access_cycles(addr: u32, width_bytes: u8, access: AccessType) -> u32 {
    access_cycles_with_waitcnt(addr, width_bytes, access, 0)
}

pub fn access_cycles_with_waitcnt(
    addr: u32,
    width_bytes: u8,
    access: AccessType,
    waitcnt: u16,
) -> u32 {
    let region = region_for_addr(addr);
    let base = match (region, access) {
        (BusRegion::Bios, _) => 1,
        (BusRegion::Ewram, _) => 3,
        (BusRegion::Iwram, _) => 1,
        (BusRegion::Io, _) => 1,
        (BusRegion::PaletteRam | BusRegion::Vram | BusRegion::Oam, _) => 1,
        (BusRegion::GamePak0, AccessType::NonSequential) => {
            gamepak_first_access_cycles(waitcnt, WAITCNT_GAMEPAK0_FIRST_SHIFT)
        }
        (BusRegion::GamePak0, AccessType::Sequential) => gamepak_second_access_cycles(
            waitcnt,
            WAITCNT_GAMEPAK0_SECOND_SHIFT,
            GAMEPAK0_SECOND_CYCLES,
        ),
        (BusRegion::GamePak1, AccessType::NonSequential) => {
            gamepak_first_access_cycles(waitcnt, WAITCNT_GAMEPAK1_FIRST_SHIFT)
        }
        (BusRegion::GamePak1, AccessType::Sequential) => gamepak_second_access_cycles(
            waitcnt,
            WAITCNT_GAMEPAK1_SECOND_SHIFT,
            GAMEPAK1_SECOND_CYCLES,
        ),
        (BusRegion::GamePak2, AccessType::NonSequential) => {
            gamepak_first_access_cycles(waitcnt, WAITCNT_GAMEPAK2_FIRST_SHIFT)
        }
        (BusRegion::GamePak2, AccessType::Sequential) => gamepak_second_access_cycles(
            waitcnt,
            WAITCNT_GAMEPAK2_SECOND_SHIFT,
            GAMEPAK2_SECOND_CYCLES,
        ),
        (BusRegion::Sram, _) => sram_access_cycles(waitcnt),
        (BusRegion::Unused, _) => 1,
    };

    if width_bytes >= WORD_BYTES
        && matches!(
            region,
            BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2
        )
    {
        let second_halfword = if addr & 0x01FF_FFFF <= 0x01FF_FFFD {
            match region {
                BusRegion::GamePak0 => gamepak_second_access_cycles(
                    waitcnt,
                    WAITCNT_GAMEPAK0_SECOND_SHIFT,
                    GAMEPAK0_SECOND_CYCLES,
                ),
                BusRegion::GamePak1 => gamepak_second_access_cycles(
                    waitcnt,
                    WAITCNT_GAMEPAK1_SECOND_SHIFT,
                    GAMEPAK1_SECOND_CYCLES,
                ),
                BusRegion::GamePak2 => gamepak_second_access_cycles(
                    waitcnt,
                    WAITCNT_GAMEPAK2_SECOND_SHIFT,
                    GAMEPAK2_SECOND_CYCLES,
                ),
                _ => unreachable!(),
            }
        } else {
            sequential_cycles_with_waitcnt(addr + HALFWORD_BYTES, waitcnt)
        };
        base + second_halfword
    } else {
        base
    }
}

pub fn instruction_fetch_cycles(addr: u32, width_bytes: u8, sequential: bool) -> u32 {
    instruction_fetch_cycles_with_waitcnt(addr, width_bytes, sequential, 0)
}

pub fn instruction_fetch_cycles_with_waitcnt(
    addr: u32,
    width_bytes: u8,
    sequential: bool,
    waitcnt: u16,
) -> u32 {
    access_cycles_with_waitcnt(
        addr,
        width_bytes,
        if sequential {
            AccessType::Sequential
        } else {
            AccessType::NonSequential
        },
        waitcnt,
    )
}

fn sequential_cycles_with_waitcnt(addr: u32, waitcnt: u16) -> u32 {
    access_cycles_with_waitcnt(addr, HALFWORD_BYTES as u8, AccessType::Sequential, waitcnt)
}

fn gamepak_first_access_cycles(waitcnt: u16, shift: u16) -> u32 {
    ACCESS_CYCLE_TABLE[((waitcnt >> shift) & WAITCNT_MASK_2BIT) as usize]
}

fn gamepak_second_access_cycles(waitcnt: u16, shift: u16, cycles: [u32; 2]) -> u32 {
    cycles[((waitcnt >> shift) & WAITCNT_MASK_1BIT) as usize]
}

fn sram_access_cycles(waitcnt: u16) -> u32 {
    ACCESS_CYCLE_TABLE[((waitcnt >> WAITCNT_SRAM_SHIFT) & WAITCNT_MASK_2BIT) as usize]
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

    #[test]
    fn waitcnt_controls_gamepak0_waitstates() {
        let waitcnt = 0b10 << 2 | 1 << 4;
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0800_0000, 2, false, waitcnt),
            3
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0800_0002, 2, true, waitcnt),
            2
        );
    }

    #[test]
    fn waitcnt_controls_gamepak_mirror_waitstates() {
        let waitcnt = (0b01 << 5) | (1 << 7) | (0b11 << 8);
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0A00_0000, 2, false, waitcnt),
            4
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0A00_0002, 2, true, waitcnt),
            2
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0C00_0000, 2, false, waitcnt),
            9
        );
        assert_eq!(
            instruction_fetch_cycles_with_waitcnt(0x0C00_0002, 2, true, waitcnt),
            9
        );
    }

    #[test]
    fn word_gamepak_access_adds_sequential_halfword() {
        let waitcnt = (0b10 << 2) | (1 << 4);
        assert_eq!(
            access_cycles_with_waitcnt(0x0800_0000, 4, AccessType::NonSequential, waitcnt),
            5
        );
    }

    #[test]
    fn word_gamepak_access_preserves_waitstate_window_crossing() {
        assert_eq!(
            access_cycles_with_waitcnt(
                0x09FF_FFFE,
                4,
                AccessType::NonSequential,
                1 << WAITCNT_GAMEPAK1_SECOND_SHIFT,
            ),
            7
        );
    }
}
