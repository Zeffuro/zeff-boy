pub const SCREEN_WIDTH: usize = zeff_emu_common::system::WS_SCREEN_SIZE.0 as usize;
pub const SCREEN_HEIGHT: usize = zeff_emu_common::system::WS_SCREEN_SIZE.1 as usize;
pub const RGBA_BYTES_PER_PIXEL: usize = zeff_emu_common::system::RGBA_BYTES_PER_PIXEL;
pub const FRAMEBUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT * RGBA_BYTES_PER_PIXEL;

pub const CPU_CLOCK_HZ: u32 = 3_072_000;
pub const CYCLES_PER_SCANLINE: u32 = 256;
pub const SCANLINES_PER_FRAME: u16 = 159;
pub const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * SCANLINES_PER_FRAME as u32;
pub const FRAME_RATE_HZ: f64 = CPU_CLOCK_HZ as f64 / CYCLES_PER_FRAME as f64;
pub const FPS: f64 = FRAME_RATE_HZ;
pub const WS_FRAME_DURATION_NS: u64 = zeff_emu_common::system::WS_FRAME_DURATION_NS;
pub const WS_DEFAULT_HOST_SAMPLE_RATE_HZ: u32 = 48_000;

pub const ADDRESS_MASK: u32 = 0x000F_FFFF;
pub const WS_INTERNAL_RAM_SIZE: usize = 0x4000;
pub const WSC_INTERNAL_RAM_SIZE: usize = 0x1_0000;
pub const IO_PORT_COUNT: usize = 0x1_0000;
pub const ROM_BANK_SIZE: usize = 0x1_0000;
pub const LINEAR_ROM_WINDOW_SIZE: usize = 0xC_0000;
