pub const SCREEN_WIDTH: usize = 224;
pub const SCREEN_HEIGHT: usize = 144;
pub const FRAMEBUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

pub const CPU_CLOCK_HZ: u32 = 3_072_000;
pub const FPS: f64 = 75.47;
pub const CYCLES_PER_SCANLINE: u32 = 256;
pub const SCANLINES_PER_FRAME: u16 = 159;
pub const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * SCANLINES_PER_FRAME as u32;
pub const WS_FRAME_DURATION_NS: u64 = 13_250_298;

pub const ADDRESS_MASK: u32 = 0x000F_FFFF;
pub const INTERNAL_RAM_SIZE: usize = 0x1_0000;
pub const IO_PORT_COUNT: usize = 0x1_0000;
pub const ROM_BANK_SIZE: usize = 0x1_0000;
pub const LINEAR_ROM_WINDOW_SIZE: usize = 0xC_0000;
