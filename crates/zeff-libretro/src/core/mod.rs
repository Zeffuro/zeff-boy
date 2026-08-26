pub(crate) enum ActiveCore {
    Gb(Box<zeff_gb_core::emulator::Emulator>),
    Gba(Box<zeff_gba_core::emulator::Emulator>),
    Nes(Box<zeff_nes_core::emulator::Emulator>),
    Pce(Box<zeff_pce_core::hardware::PceHuCardHost>),
    Sega8(Box<zeff_sega8_core::emulator::Emulator>),
    Ws(Box<zeff_ws_core::emulator::Emulator>),
}

pub(crate) const LIBRETRO_DEFAULT_OUTPUT_SAMPLE_RATE_HZ: u32 =
    zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE;
pub(crate) const LIBRETRO_RGB565_BYTES_PER_PIXEL: usize = 2;

pub(crate) struct CoreState {
    pub core: ActiveCore,
    pub rom_data: Vec<u8>,
    pub ram_cheats: Vec<zeff_emu_common::cheats::CheatPatch>,
    pub gba_codebreaker_state: zeff_emu_common::cheats::GbaCodeBreakerState,
    pub audio_buf: Vec<f32>,
    pub sample_rate: u32,
    pub xrgb_buf: Vec<u8>,
    pub rgb565_buf: Vec<u8>,
    pub system_ram_buf: Vec<u8>,
    pub video_ram_buf: Vec<u8>,
    pub port_device: [u32; 2],
}

mod cheats;
mod lifecycle;
mod memory;
mod runtime;
mod video;

#[cfg(test)]
mod tests;
