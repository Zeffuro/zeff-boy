use std::path::{Path, PathBuf};

use anyhow::Context;

pub(crate) use self::gb::GbBackend;
pub(crate) use self::gba::GbaBackend;
pub(crate) use self::nes::NesBackend;
pub(crate) use self::ws::WsBackend;

use crate::emu_core_trait::EmulatorCore;

pub(crate) mod gb;
pub(crate) mod gba;
pub(crate) mod nes;
pub(crate) mod ws;

pub(crate) const ROM_EXTENSIONS: &[&str] = &["gb", "gbc", "sgb", "gba", "nes", "ws", "wsc"];
pub(crate) const ROM_AND_ARCHIVE_EXTENSIONS: &[&str] =
    &["gb", "gbc", "sgb", "gba", "nes", "ws", "wsc", "zip"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActiveSystem {
    GameBoy,
    GameBoyAdvance,
    Nes,
    WonderSwan,
}

impl ActiveSystem {
    pub(crate) fn storage_subdir(self) -> &'static str {
        match self {
            Self::GameBoy => "gbc",
            Self::GameBoyAdvance => "gba",
            Self::Nes => "nes",
            Self::WonderSwan => "ws",
        }
    }

    pub(crate) fn screen_size(self) -> (u32, u32) {
        match self {
            Self::GameBoy => (160, 144),
            Self::GameBoyAdvance => (240, 160),
            Self::Nes => (256, 240),
            Self::WonderSwan => (224, 144),
        }
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "gb" | "gbc" | "sgb" => Some(Self::GameBoy),
            "gba" => Some(Self::GameBoyAdvance),
            "nes" => Some(Self::Nes),
            "ws" | "wsc" => Some(Self::WonderSwan),
            _ => None,
        }
    }

    pub(crate) fn supported_extensions() -> &'static str {
        ".gb, .gbc, .sgb, .gba, .nes, .ws, .wsc, .zip"
    }
}

pub(crate) enum EmuBackend {
    Gb(Box<GbBackend>),
    Gba(Box<GbaBackend>),
    Nes(Box<NesBackend>),
    Ws(Box<WsBackend>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            EmuBackend::Gb(b) => b.$method($($arg),*),
            EmuBackend::Gba(b) => b.$method($($arg),*),
            EmuBackend::Nes(b) => b.$method($($arg),*),
            EmuBackend::Ws(b) => b.$method($($arg),*),
        }
    };
}

impl EmuBackend {
    pub(crate) fn from_gb(emu: zeff_gb_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Gb(Box::new(GbBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_gb_with_source(
        emu: zeff_gb_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Gb(Box::new(GbBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn from_gba(emu: zeff_gba_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Gba(Box::new(GbaBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_gba_with_source(
        emu: zeff_gba_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Gba(Box::new(GbaBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn from_nes(emu: zeff_nes_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Nes(Box::new(NesBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_nes_with_source(
        emu: zeff_nes_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Nes(Box::new(NesBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn from_ws(emu: zeff_ws_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Ws(Box::new(WsBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_ws_with_source(
        emu: zeff_ws_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Ws(Box::new(WsBackend::with_source_path(
            emu,
            rom_path,
            source_path,
        )))
    }

    pub(crate) fn system(&self) -> ActiveSystem {
        match self {
            Self::Gb(..) => ActiveSystem::GameBoy,
            Self::Gba(..) => ActiveSystem::GameBoyAdvance,
            Self::Nes(..) => ActiveSystem::Nes,
            Self::Ws(..) => ActiveSystem::WonderSwan,
        }
    }

    fn state_extension(&self) -> &'static str {
        match self {
            Self::Gb(..) => "gbstate",
            Self::Gba(..) => "gbastate",
            Self::Nes(..) => "nstate",
            Self::Ws(..) => "wsstate",
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

    pub(crate) fn gba_mut(&mut self) -> Option<&mut GbaBackend> {
        match self {
            Self::Gba(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn gba(&self) -> Option<&GbaBackend> {
        match self {
            Self::Gba(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn nes(&self) -> Option<&NesBackend> {
        match self {
            Self::Nes(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn ws_mut(&mut self) -> Option<&mut WsBackend> {
        match self {
            Self::Ws(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn ws(&self) -> Option<&WsBackend> {
        match self {
            Self::Ws(b) => Some(b),
            _ => None,
        }
    }
}

impl EmuBackend {
    #[inline]
    pub(crate) fn framebuffer(&self) -> &[u8] {
        dispatch!(self, framebuffer())
    }

    #[inline]
    pub(crate) fn is_suspended(&self) -> bool {
        dispatch!(self, is_suspended())
    }

    pub(crate) fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        dispatch!(self, encode_state_bytes())
    }

    pub(crate) fn rom_path(&self) -> &Path {
        dispatch!(self, rom_path())
    }

    pub(crate) fn source_path(&self) -> &Path {
        dispatch!(self, source_path())
    }

    pub(crate) fn rom_hash(&self) -> [u8; 32] {
        dispatch!(self, rom_hash())
    }

    #[inline]
    pub(crate) fn rumble_active(&self) -> bool {
        dispatch!(self, rumble_active())
    }

    #[inline]
    pub(crate) fn is_mbc7(&self) -> bool {
        dispatch!(self, is_mbc7())
    }

    #[inline]
    pub(crate) fn is_pocket_camera(&self) -> bool {
        dispatch!(self, is_pocket_camera())
    }

    pub(crate) fn apu_channel_snapshot(&self) -> Option<crate::audio_recorder::MidiApuSnapshot> {
        dispatch!(self, apu_channel_snapshot())
    }

    #[inline]
    pub(crate) fn step_frame(&mut self) {
        dispatch!(self, step_frame())
    }

    #[inline]
    pub(crate) fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        dispatch!(self, drain_audio_samples_into(buf))
    }

    pub(crate) fn set_sample_rate(&mut self, rate: u32) {
        dispatch!(self, set_sample_rate(rate))
    }

    pub(crate) fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        dispatch!(self, set_apu_sample_generation_enabled(enabled))
    }

    pub(crate) fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        dispatch!(self, set_apu_channel_mutes(mutes))
    }

    #[inline]
    pub(crate) fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input(buttons_pressed, dpad_pressed))
    }

    #[inline]
    pub(crate) fn set_zapper_state(
        &mut self,
        enabled: bool,
        trigger: bool,
        hit: bool,
        screen_pos: Option<(u16, u16)>,
    ) {
        dispatch!(self, set_zapper_state(enabled, trigger, hit, screen_pos))
    }

    pub(crate) fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input_p2(buttons_pressed, dpad_pressed))
    }

    pub(crate) fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        dispatch!(self, flush_battery_sram())
    }

    pub(crate) fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        dispatch!(self, load_state_from_bytes(bytes))
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.is_suspended()
    }

    pub(crate) fn slot_path(&self, slot: u8) -> anyhow::Result<PathBuf> {
        crate::save_paths::slot_path(
            self.system().storage_subdir(),
            self.state_extension(),
            self.rom_hash(),
            slot,
        )
    }

    pub(crate) fn auto_save_path(&self) -> Option<PathBuf> {
        Some(crate::save_paths::auto_save_path(
            self.system().storage_subdir(),
            self.state_extension(),
            self.rom_hash(),
        ))
    }

    pub(crate) fn load_state(&mut self, slot: u8) -> anyhow::Result<String> {
        let path = self.slot_path(slot)?;
        let bytes = crate::platform::read_save_data(&path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("save state not found: {}", path.display()))?;
        self.load_state_from_bytes(bytes)?;
        Ok(path.display().to_string())
    }

    pub(crate) fn load_state_from_path(&mut self, path: &Path) -> anyhow::Result<()> {
        let bytes = crate::platform::read_save_data(path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("save state not found: {}", path.display()))?;
        self.load_state_from_bytes(bytes)
    }
}

#[cfg(test)]
mod tests;
