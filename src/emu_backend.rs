use std::path::{Path, PathBuf};

use anyhow::Context;

pub(crate) use self::gb::GbBackend;
pub(crate) use self::nes::NesBackend;

use crate::emu_core_trait::EmulatorCore;

pub(crate) mod gb;
pub(crate) mod nes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActiveSystem {
    GameBoy,
    Nes,
}

impl ActiveSystem {
    pub(crate) fn storage_subdir(self) -> &'static str {
        match self {
            Self::GameBoy => "gbc",
            Self::Nes => "nes",
        }
    }

    pub(crate) fn screen_size(self) -> (u32, u32) {
        match self {
            Self::GameBoy => (160, 144),
            Self::Nes => (256, 240),
        }
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "gb" | "gbc" | "sgb" => Some(Self::GameBoy),
            "nes" => Some(Self::Nes),
            _ => None,
        }
    }

    pub(crate) fn supported_extensions() -> &'static str {
        ".gb, .gbc, .sgb, .nes"
    }
}

pub(crate) enum EmuBackend {
    Gb(Box<GbBackend>),
    Nes(Box<NesBackend>),
}

impl EmuBackend {
    pub(crate) fn from_gb(emu: zeff_gb_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Gb(Box::new(GbBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_nes(emu: zeff_nes_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Nes(Box::new(NesBackend::new(emu, rom_path)))
    }

    pub(crate) fn system(&self) -> ActiveSystem {
        match self {
            Self::Gb(..) => ActiveSystem::GameBoy,
            Self::Nes(..) => ActiveSystem::Nes,
        }
    }

    fn state_extension(&self) -> &'static str {
        match self {
            Self::Gb(..) => "gbstate",
            Self::Nes(..) => "nstate",
        }
    }

    pub(crate) fn core(&self) -> &dyn EmulatorCore {
        match self {
            Self::Gb(b) => &**b,
            Self::Nes(b) => &**b,
        }
    }

    pub(crate) fn core_mut(&mut self) -> &mut dyn EmulatorCore {
        match self {
            Self::Gb(b) => &mut **b,
            Self::Nes(b) => &mut **b,
        }
    }

    pub(crate) fn gb(&self) -> Option<&GbBackend> {
        match self {
            Self::Gb(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn gb_mut(&mut self) -> Option<&mut GbBackend> {
        match self {
            Self::Gb(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn nes_mut(&mut self) -> Option<&mut NesBackend> {
        match self {
            Self::Nes(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn nes(&self) -> Option<&NesBackend> {
        match self {
            Self::Nes(b) => Some(b),
            _ => None,
        }
    }
}

impl EmuBackend {
    pub(crate) fn step_frame(&mut self) {
        self.core_mut().step_frame();
    }
    pub(crate) fn framebuffer(&self) -> &[u8] {
        self.core().framebuffer()
    }
    pub(crate) fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.core_mut().drain_audio_samples()
    }
    pub(crate) fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.core_mut().drain_audio_samples_into(buf);
    }
    pub(crate) fn set_sample_rate(&mut self, rate: u32) {
        self.core_mut().set_sample_rate(rate);
    }
    pub(crate) fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.core_mut().set_apu_sample_generation_enabled(enabled);
    }
    pub(crate) fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        self.core_mut().set_apu_channel_mutes(mutes);
    }
    pub(crate) fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.core_mut().set_input(buttons_pressed, dpad_pressed);
    }
    pub(crate) fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.core_mut().set_input_p2(buttons_pressed, dpad_pressed);
    }
    pub(crate) fn is_suspended(&self) -> bool {
        self.core().is_suspended()
    }
    pub(crate) fn is_running(&self) -> bool {
        !self.core().is_suspended()
    }
    pub(crate) fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        self.core_mut().flush_battery_sram()
    }
    pub(crate) fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.core().encode_state_bytes()
    }
    pub(crate) fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.core_mut().load_state_from_bytes(bytes)
    }
    pub(crate) fn rom_path(&self) -> &Path {
        self.core().rom_path()
    }
    pub(crate) fn rom_hash(&self) -> [u8; 32] {
        self.core().rom_hash()
    }
    pub(crate) fn rumble_active(&self) -> bool {
        self.core().rumble_active()
    }
    pub(crate) fn is_mbc7(&self) -> bool {
        self.core().is_mbc7()
    }
    pub(crate) fn is_pocket_camera(&self) -> bool {
        self.core().is_pocket_camera()
    }
    pub(crate) fn apu_channel_snapshot(&self) -> Option<crate::audio_recorder::MidiApuSnapshot> {
        self.core().apu_channel_snapshot()
    }

    pub(crate) fn slot_path(&self, slot: u8) -> anyhow::Result<PathBuf> {
        crate::save_paths::slot_path(
            self.system().storage_subdir(),
            self.state_extension(),
            self.core().rom_hash(),
            slot,
        )
    }

    pub(crate) fn auto_save_path(&self) -> Option<PathBuf> {
        Some(crate::save_paths::auto_save_path(
            self.system().storage_subdir(),
            self.state_extension(),
            self.core().rom_hash(),
        ))
    }

    pub(crate) fn load_state(&mut self, slot: u8) -> anyhow::Result<String> {
        let path = self.slot_path(slot)?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?;
        self.core_mut().load_state_from_bytes(bytes)?;
        Ok(path.display().to_string())
    }

    pub(crate) fn load_state_from_path(&mut self, path: &Path) -> anyhow::Result<()> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?;
        self.core_mut().load_state_from_bytes(bytes)
    }
}

#[cfg(test)]
mod tests;
