pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
pub const FRAMEBUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

pub const CPU_CLOCK_HZ: u32 = 16_777_216;
pub const FPS: f64 = 59.7275;
pub const CYCLES_PER_FRAME: u32 = 280_896;

pub const BIOS_SIZE: usize = 0x4000;
pub const EWRAM_SIZE: usize = 0x40000;
pub const IWRAM_SIZE: usize = 0x8000;
pub const IO_SIZE: usize = 0x400;
pub const PALETTE_RAM_SIZE: usize = 0x400;
pub const VRAM_SIZE: usize = 0x18000;
pub const OAM_SIZE: usize = 0x400;
pub const SRAM_SIZE: usize = 0x10000;
pub const FLASH_1M_SIZE: usize = 0x20000;
pub const EEPROM_SIZE: usize = 0x2000;
