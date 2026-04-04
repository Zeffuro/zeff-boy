use std::path::{Path, PathBuf};

use zeff_nes_core::emulator::Emulator as NesEmulator;

use crate::audio_recorder::MidiApuSnapshot;
use crate::emu_core_trait::EmulatorCore;

pub(crate) struct NesBackend {
    pub(crate) emu: NesEmulator,
    rom_path: PathBuf,
}

impl NesBackend {
    pub(crate) fn new(emu: NesEmulator, rom_path: PathBuf) -> Self {
        Self { emu, rom_path }
    }
}

fn map_host_to_nes_byte(buttons_pressed: u8, dpad_pressed: u8) -> u8 {
    (buttons_pressed & 0x0F)
        | ((dpad_pressed & 0x04) << 2) // Up
        | ((dpad_pressed & 0x08) << 2) // Down
        | ((dpad_pressed & 0x02) << 5) // Left
        | ((dpad_pressed & 0x01) << 7) // Right
}

impl EmulatorCore for NesBackend {
    fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_into_stereo(buf);
    }

    fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mono = self.emu.drain_audio_samples();
        let mut stereo = Vec::with_capacity(mono.len() * 2);
        for &sample in &mono {
            stereo.push(sample);
            stereo.push(sample);
        }
        stereo
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let arr: [bool; 5] = std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu
            .set_input_p1(map_host_to_nes_byte(buttons_pressed, dpad_pressed));
    }

    fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu
            .set_input_p2(map_host_to_nes_byte(buttons_pressed, dpad_pressed));
    }

    fn is_suspended(&self) -> bool {
        self.emu.is_cpu_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        crate::save_paths::flush_battery_sram(&self.rom_path, self.emu.dump_battery_sram())
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state()
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.emu.load_state_from_bytes(bytes)
    }

    fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.emu.rom_hash()
    }

    fn apu_channel_snapshot(&self) -> Option<MidiApuSnapshot> {
        Some(MidiApuSnapshot::Nes(self.emu.apu_channel_snapshot()))
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut NesEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "NES", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}

#[cfg(test)]
mod tests;
