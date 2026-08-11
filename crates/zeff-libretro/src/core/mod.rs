pub(crate) enum ActiveCore {
    Gb(Box<zeff_gb_core::emulator::Emulator>),
    Gba(Box<zeff_gba_core::emulator::Emulator>),
    Nes(Box<zeff_nes_core::emulator::Emulator>),
    Sega8(Box<zeff_sega8_core::emulator::Emulator>),
    Ws(Box<zeff_ws_core::emulator::Emulator>),
}

pub(crate) struct CoreState {
    pub core: ActiveCore,
    pub rom_data: Vec<u8>,
    pub ram_cheats: Vec<zeff_emu_common::cheats::CheatPatch>,
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
