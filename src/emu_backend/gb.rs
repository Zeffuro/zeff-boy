use std::path::{Path, PathBuf};

use zeff_gb_core::emulator::Emulator as GbEmulator;

use crate::audio_recorder::MidiApuSnapshot;
use crate::emu_core_trait::EmulatorCore;

pub(crate) struct GbBackend {
    pub(crate) emu: GbEmulator,
    rom_path: PathBuf,
}

impl GbBackend {
    pub(crate) fn new(emu: GbEmulator, rom_path: PathBuf) -> Self {
        Self { emu, rom_path }
    }
}

impl EmulatorCore for GbBackend {
    fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(buf);
    }

    fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.emu.drain_audio_samples()
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let arr = [
            mutes.first().copied().unwrap_or(false),
            mutes.get(1).copied().unwrap_or(false),
            mutes.get(2).copied().unwrap_or(false),
            mutes.get(3).copied().unwrap_or(false),
        ];
        self.emu.set_apu_channel_mutes(arr);
    }

    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    fn is_suspended(&self) -> bool {
        self.emu.is_cpu_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        let Some(bytes) = self.emu.dump_battery_sram() else {
            return Ok(None);
        };
        let save_path = sram_path_for_rom(&self.rom_path);
        crate::save_paths::write_sram_file(&save_path, &bytes)?;
        Ok(Some(save_path.display().to_string()))
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state_bytes()
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

    fn screen_size(&self) -> (u32, u32) {
        (160, 144)
    }

    fn storage_subdir(&self) -> &'static str {
        "gbc"
    }

    fn state_extension(&self) -> &'static str {
        "gbstate"
    }

    fn apu_channel_snapshot(&self) -> Option<MidiApuSnapshot> {
        Some(MidiApuSnapshot::Gb(self.emu.apu_channel_snapshot()))
    }

    fn rumble_active(&self) -> bool {
        self.emu.rumble_active()
    }

    fn is_mbc7(&self) -> bool {
        self.emu.is_mbc7_cartridge()
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut GbEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    if !emu.is_battery_backed() {
        return Ok(None);
    }
    let save_path = sram_path_for_rom(rom_path);
    if !save_path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&save_path)
        .map_err(|e| anyhow::anyhow!("failed to read GB save {}: {e}", save_path.display()))?;
    emu.load_battery_sram(&bytes)?;
    Ok(Some(save_path.display().to_string()))
}

fn sram_path_for_rom(rom_path: &Path) -> PathBuf {
    rom_path.with_extension("sav")
}
