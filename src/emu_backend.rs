use std::path::{Path, PathBuf};

use zeff_gb_core::emulator::Emulator as GbEmulator;
use zeff_nes_core::emulator::Emulator as NesEmulator;

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

#[allow(clippy::large_enum_variant)]
pub(crate) enum EmuBackend {
    Gb(GbEmulator, PathBuf),
    Nes(NesEmulator, PathBuf),
}

impl EmuBackend {
    pub(crate) fn system(&self) -> ActiveSystem {
        match self {
            Self::Gb(..) => ActiveSystem::GameBoy,
            Self::Nes(..) => ActiveSystem::Nes,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn screen_size(&self) -> (u32, u32) {
        self.system().screen_size()
    }

    pub(crate) fn from_gb(emu: GbEmulator, rom_path: PathBuf) -> Self {
        Self::Gb(emu, rom_path)
    }

    pub(crate) fn from_nes(emu: NesEmulator, rom_path: PathBuf) -> Self {
        Self::Nes(emu, rom_path)
    }

    pub(crate) fn gb(&self) -> Option<&GbEmulator> {
        match self {
            Self::Gb(e, _) => Some(e),
            _ => None,
        }
    }

    pub(crate) fn gb_mut(&mut self) -> Option<&mut GbEmulator> {
        match self {
            Self::Gb(e, _) => Some(e),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn nes_mut(&mut self) -> Option<&mut NesEmulator> {
        match self {
            Self::Nes(e, _) => Some(e),
            _ => None,
        }
    }

    pub(crate) fn step_frame(&mut self) {
        match self {
            Self::Gb(e, _) => e.step_frame(),
            Self::Nes(e, _) => e.step_frame(),
        }
    }

    pub(crate) fn framebuffer(&self) -> &[u8] {
        match self {
            Self::Gb(e, _) => e.framebuffer(),
            Self::Nes(e, _) => e.framebuffer(),
        }
    }

    pub(crate) fn drain_audio_samples(&mut self) -> Vec<f32> {
        match self {
            Self::Gb(e, _) => gb::drain_audio_samples(e),
            Self::Nes(e, _) => nes::drain_audio_samples(e),
        }
    }

    pub(crate) fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        match self {
            Self::Gb(e, _) => gb::drain_audio_samples_into(e, buf),
            Self::Nes(e, _) => nes::drain_audio_samples_into(e, buf),
        }
    }

    pub(crate) fn set_sample_rate(&mut self, rate: u32) {
        match self {
            Self::Gb(e, _) => gb::set_sample_rate(e, rate),
            Self::Nes(e, _) => nes::set_sample_rate(e, rate),
        }
    }

    pub(crate) fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        match self {
            Self::Gb(e, _) => gb::set_apu_sample_generation_enabled(e, enabled),
            Self::Nes(e, _) => nes::set_apu_sample_generation_enabled(e, enabled),
        }
    }

    pub(crate) fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        match self {
            Self::Gb(e, _) => gb::set_apu_channel_mutes(e, mutes),
            Self::Nes(e, _) => nes::set_apu_channel_mutes(e, mutes),
        }
    }

    pub(crate) fn rom_path(&self) -> &Path {
        match self {
            Self::Gb(_, p) => p,
            Self::Nes(_, p) => p,
        }
    }

    pub(crate) fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        match self {
            Self::Gb(e, _) => gb::set_input(e, buttons_pressed, dpad_pressed),
            Self::Nes(e, _) => nes::set_input(e, buttons_pressed, dpad_pressed),
        }
    }

    pub(crate) fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        match self {
            Self::Gb(..) => {}
            Self::Nes(e, _) => nes::set_input_p2(e, buttons_pressed, dpad_pressed),
        }
    }

    pub(crate) fn is_suspended(&self) -> bool {
        match self {
            Self::Gb(e, _) => e.is_cpu_suspended(),
            Self::Nes(e, _) => e.is_cpu_suspended(),
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.is_suspended()
    }

    pub(crate) fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        match self {
            Self::Gb(e, p) => gb::flush_battery_sram(e, p),
            Self::Nes(e, p) => nes::flush_battery_sram(e, p),
        }
    }

    pub(crate) fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        match self {
            Self::Gb(e, _) => gb::encode_state_bytes(e),
            Self::Nes(e, _) => nes::encode_state_bytes(e),
        }
    }

    pub(crate) fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        match self {
            Self::Gb(e, _) => gb::load_state_from_bytes(e, bytes),
            Self::Nes(e, _) => nes::load_state_from_bytes(e, bytes),
        }
    }

    pub(crate) fn slot_path(&self, slot: u8) -> anyhow::Result<std::path::PathBuf> {
        match self {
            Self::Gb(e, _) => gb::slot_path(e, slot),
            Self::Nes(e, _) => nes::slot_path(e, slot),
        }
    }

    pub(crate) fn auto_save_path(&self) -> Option<std::path::PathBuf> {
        match self {
            Self::Gb(e, _) => gb::auto_save_path(e),
            Self::Nes(e, _) => nes::auto_save_path(e),
        }
    }

    pub(crate) fn load_state(&mut self, slot: u8) -> anyhow::Result<String> {
        match self {
            Self::Gb(e, _) => gb::load_state(e, slot),
            Self::Nes(e, _) => nes::load_state(e, slot),
        }
    }

    pub(crate) fn load_state_from_path(&mut self, path: &Path) -> anyhow::Result<()> {
        match self {
            Self::Gb(e, _) => gb::load_state_from_path(e, path),
            Self::Nes(e, _) => nes::load_state_from_path(e, path),
        }
    }

    pub(crate) fn rom_hash(&self) -> Option<[u8; 32]> {
        match self {
            Self::Gb(e, _) => Some(e.rom_hash()),
            Self::Nes(e, _) => Some(e.rom_hash()),
        }
    }

    pub(crate) fn rumble_active(&self) -> bool {
        match self {
            Self::Gb(e, _) => gb::rumble_active(e),
            Self::Nes(..) => false,
        }
    }

    pub(crate) fn is_mbc7(&self) -> bool {
        match self {
            Self::Gb(e, _) => gb::is_mbc7(e),
            Self::Nes(..) => false,
        }
    }

    pub(crate) fn apu_channel_snapshot(&self) -> Option<crate::audio_recorder::MidiApuSnapshot> {
        match self {
            Self::Gb(e, _) => gb::apu_channel_snapshot(e),
            Self::Nes(e, _) => nes::apu_channel_snapshot(e),
        }
    }
}

