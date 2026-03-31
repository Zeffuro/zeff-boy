use std::path::{Path, PathBuf};

use zeff_gb_core::emulator::Emulator as GbEmulator;

use crate::audio_recorder::MidiApuSnapshot;

pub(crate) struct GbBackend {
    pub(crate) emu: GbEmulator,
    rom_path: PathBuf,
}

impl GbBackend {
    pub(crate) fn new(emu: GbEmulator, rom_path: PathBuf) -> Self {
        Self { emu, rom_path }
    }

    pub(crate) fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    pub(crate) fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    pub(crate) fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(buf);
    }

    pub(crate) fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.emu.drain_audio_samples()
    }

    pub(crate) fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    pub(crate) fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }

    pub(crate) fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let arr = [
            mutes.first().copied().unwrap_or(false),
            mutes.get(1).copied().unwrap_or(false),
            mutes.get(2).copied().unwrap_or(false),
            mutes.get(3).copied().unwrap_or(false),
        ];
        self.emu.set_apu_channel_mutes(arr);
    }

    pub(crate) fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    pub(crate) fn set_input_p2(&mut self, _buttons_pressed: u8, _dpad_pressed: u8) {}

    pub(crate) fn is_suspended(&self) -> bool {
        self.emu.is_cpu_suspended()
    }

    pub(crate) fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        crate::save_paths::flush_battery_sram(&self.rom_path, self.emu.dump_battery_sram())
    }

    pub(crate) fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state_bytes()
    }

    pub(crate) fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.emu.load_state_from_bytes(bytes)
    }

    pub(crate) fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    pub(crate) fn rom_hash(&self) -> [u8; 32] {
        self.emu.rom_hash()
    }

    pub(crate) fn apu_channel_snapshot(&self) -> Option<MidiApuSnapshot> {
        Some(MidiApuSnapshot::Gb(self.emu.apu_channel_snapshot()))
    }

    pub(crate) fn rumble_active(&self) -> bool {
        self.emu.rumble_active()
    }

    pub(crate) fn is_mbc7(&self) -> bool {
        self.emu.is_mbc7_cartridge()
    }

    pub(crate) fn is_pocket_camera(&self) -> bool {
        self.emu.is_pocket_camera_cartridge()
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut GbEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "GB", emu.is_battery_backed(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
