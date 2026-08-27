pub const CPU_CLOCK_HZ: u64 = 3_579_545;
pub const CPU_CYCLES_PER_SCANLINE: u32 = 228;
pub const SCANLINES_PER_FRAME: u16 = 262;
pub const CPU_CYCLES_PER_FRAME: u64 = CPU_CYCLES_PER_SCANLINE as u64 * SCANLINES_PER_FRAME as u64;

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 192;
pub const FRAMEBUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;
pub const BIOS_SIZE: usize = 8 * 1024;
pub const WORK_RAM_SIZE: usize = 1024;
pub const VRAM_SIZE: usize = 16 * 1024;

pub(crate) const BIOS_START: u16 = 0x0000;
pub(crate) const BIOS_END: u16 = 0x1FFF;
pub(crate) const EXPANSION_START: u16 = 0x2000;
pub(crate) const EXPANSION_END: u16 = 0x5FFF;
pub(crate) const WORK_RAM_START: u16 = 0x6000;
pub(crate) const WORK_RAM_END: u16 = 0x7FFF;
pub(crate) const CARTRIDGE_START: u16 = 0x8000;
pub(crate) const CARTRIDGE_END: u16 = 0xFFFF;
pub(crate) const MEMORY_OPEN_BUS_VALUE: u8 = 0xFF;

pub(crate) const KEYPAD_MODE_PORT_START: u8 = 0x80;
pub(crate) const KEYPAD_MODE_PORT_END: u8 = 0x9F;
pub(crate) const VDP_PORT_START: u8 = 0xA0;
pub(crate) const VDP_PORT_END: u8 = 0xBF;
pub(crate) const JOYSTICK_MODE_PORT_START: u8 = 0xC0;
pub(crate) const JOYSTICK_MODE_PORT_END: u8 = 0xDF;
pub(crate) const CONTROLLER_PSG_PORT_START: u8 = 0xE0;
pub(crate) const CONTROLLER_PSG_PORT_END: u8 = 0xFF;
pub(crate) const IO_OPEN_BUS_VALUE: u8 = 0xFF;

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
pub const MAX_CARTRIDGE_SIZE: usize = 32 * 1024;
