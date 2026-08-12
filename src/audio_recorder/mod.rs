mod midi;

#[cfg(test)]
mod tests;

use zeff_gb_core::hardware::apu::ApuChannelSnapshot as GbApuChannelSnapshot;
use zeff_nes_core::hardware::apu::ApuChannelSnapshot as NesApuChannelSnapshot;
use zeff_sega8_core::hardware::apu::ApuDebugSnapshot as Sega8ApuSnapshot;

pub(crate) fn ogg_vorbis_supported() -> bool {
    cfg!(feature = "audio-recording")
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum MidiApuSnapshot {
    Gb(GbApuChannelSnapshot),
    Nes(NesApuChannelSnapshot),
    Sega8(Sega8ApuSnapshot),
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::AudioRecorder;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::AudioRecorder;
