pub const SCREEN_WIDTH: usize = zeff_emu_common::system::GBA_SCREEN_SIZE.0 as usize;
pub const SCREEN_HEIGHT: usize = zeff_emu_common::system::GBA_SCREEN_SIZE.1 as usize;
pub const FRAMEBUFFER_LEN: usize =
    SCREEN_WIDTH * SCREEN_HEIGHT * zeff_emu_common::system::RGBA_BYTES_PER_PIXEL;

pub const CPU_CLOCK_HZ: u32 = 16_777_216;
pub const PSG_CLOCK_HZ: u32 = CPU_CLOCK_HZ / 4;
pub const CYCLES_PER_SCANLINE: u32 = 1232;
pub const HBLANK_START_CYCLE: u32 = 1006;
pub const SCANLINES_PER_FRAME: u16 = 228;
pub const VISIBLE_SCANLINES: u16 = SCREEN_HEIGHT as u16;
pub const VBLANK_END_SCANLINE: u16 = SCANLINES_PER_FRAME - 1;
pub const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * SCANLINES_PER_FRAME as u32;
pub const FRAME_RATE_HZ: f64 = CPU_CLOCK_HZ as f64 / CYCLES_PER_FRAME as f64;
pub const FPS: f64 = FRAME_RATE_HZ;
pub const GBA_DEFAULT_HOST_SAMPLE_RATE_HZ: u32 = 48_000;

pub const BIOS_SIZE: usize = 0x4000;
pub const EWRAM_SIZE: usize = 0x40000;
pub const IWRAM_SIZE: usize = 0x8000;
pub const SYSTEM_RAM_SIZE: usize = EWRAM_SIZE + IWRAM_SIZE;
pub const IO_SIZE: usize = 0x400;
pub const PALETTE_RAM_SIZE: usize = 0x400;
pub const VRAM_SIZE: usize = 0x18000;
pub const OAM_SIZE: usize = 0x400;
pub const SRAM_SIZE: usize = 0x10000;
pub const FLASH_1M_SIZE: usize = 0x20000;
pub const EEPROM_SIZE: usize = 0x2000;

pub(crate) const BIOS_START: u32 = 0x0000_0000;
pub(crate) const BIOS_END: u32 = 0x0000_3FFF;
pub(crate) const EWRAM_START: u32 = 0x0200_0000;
pub(crate) const EWRAM_END: u32 = 0x02FF_FFFF;
pub(crate) const IWRAM_START: u32 = 0x0300_0000;
pub(crate) const IWRAM_END: u32 = 0x03FF_FFFF;
pub(crate) const IO_START: u32 = 0x0400_0000;
pub(crate) const IO_END: u32 = 0x0400_03FF;
pub(crate) const IO_LAST_ALIGNED_HALFWORD_ADDR: u32 = IO_END - 1;
pub(crate) const IO_UNUSED_START: u32 = IO_END + 1;
pub(crate) const PALETTE_RAM_START: u32 = 0x0500_0000;
pub(crate) const PALETTE_RAM_END: u32 = 0x05FF_FFFF;
pub(crate) const VRAM_START: u32 = 0x0600_0000;
pub(crate) const VRAM_END: u32 = 0x06FF_FFFF;
pub(crate) const OAM_START: u32 = 0x0700_0000;
pub(crate) const OAM_END: u32 = 0x07FF_FFFF;
pub(crate) const GAMEPAK0_START: u32 = 0x0800_0000;
pub(crate) const GAMEPAK0_END: u32 = 0x09FF_FFFF;
pub(crate) const GAMEPAK1_START: u32 = 0x0A00_0000;
pub(crate) const GAMEPAK1_END: u32 = 0x0BFF_FFFF;
pub(crate) const GAMEPAK2_START: u32 = 0x0C00_0000;
pub(crate) const GAMEPAK2_END: u32 = 0x0DFF_FFFF;
pub(crate) const GAMEPAK_ROM_END: u32 = GAMEPAK2_END;
pub(crate) const BACKUP_START: u32 = 0x0E00_0000;
pub(crate) const BACKUP_END: u32 = 0x0FFF_FFFF;
pub(crate) const SRAM_TIMING_END: u32 = 0x0E00_FFFF;

#[cfg(test)]
mod tests {
    use super::{
        EWRAM_SIZE, FRAMEBUFFER_LEN, GBA_DEFAULT_HOST_SAMPLE_RATE_HZ, IWRAM_SIZE, SCREEN_HEIGHT,
        SCREEN_WIDTH, SYSTEM_RAM_SIZE,
    };

    #[test]
    fn geometry_and_system_ram_match_their_owners() {
        assert_eq!(
            (SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32),
            zeff_emu_common::system::GBA_SCREEN_SIZE
        );
        assert_eq!(FRAMEBUFFER_LEN, 240 * 160 * 4);
        assert_eq!(SYSTEM_RAM_SIZE, EWRAM_SIZE + IWRAM_SIZE);
        assert_eq!(SYSTEM_RAM_SIZE, 0x48000);
        assert_eq!(GBA_DEFAULT_HOST_SAMPLE_RATE_HZ, 48_000);
    }
}
