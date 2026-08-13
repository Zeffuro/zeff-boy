use std::path::{Path, PathBuf};

use anyhow::Context;
use zeff_emu_common::memory::MemoryRegionDescriptor;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::system::CoreFamily;

pub(crate) use self::capabilities::{CheatCapabilities, CoreCapabilities, InputCapabilities};
pub(crate) use self::gb::GbBackend;
pub(crate) use self::gba::GbaBackend;
pub(crate) use self::loader::{BackendLoadConfig, load_backend_from_rom_source};
pub(crate) use self::nes::NesBackend;
pub(crate) use self::runtime::BackendRuntimeConfig;
pub(crate) use self::sega8::Sega8Backend;
pub(crate) use self::system::{
    ActiveSystem, ROM_AND_ARCHIVE_EXTENSIONS, ROM_EXTENSIONS, archive_extensions, system_specs,
};
pub(crate) use self::ws::WsBackend;

use crate::emu_core_trait::EmulatorCore;

pub(crate) mod capabilities;
pub(crate) mod cheats;
pub(crate) mod firmware;
pub(crate) mod gb;
pub(crate) mod gba;
pub(crate) mod loader;
pub(crate) mod nes;
pub(crate) mod paths;
pub(crate) mod runtime;
pub(crate) mod sega8;
pub(crate) mod system;
pub(crate) mod ws;

pub(crate) enum EmuBackend {
    Gb(Box<GbBackend>),
    Gba(Box<GbaBackend>),
    Nes(Box<NesBackend>),
    Sega8(Box<Sega8Backend>),
    Ws(Box<WsBackend>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            EmuBackend::Gb(b) => b.$method($($arg),*),
            EmuBackend::Gba(b) => b.$method($($arg),*),
            EmuBackend::Nes(b) => b.$method($($arg),*),
            EmuBackend::Sega8(b) => b.$method($($arg),*),
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

    pub(crate) fn from_sega8(emu: zeff_sega8_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Sega8(Box::new(Sega8Backend::new(emu, rom_path)))
    }

    pub(crate) fn from_sega8_with_source(
        emu: zeff_sega8_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self::Sega8(Box::new(Sega8Backend::with_source_path(
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
            Self::Sega8(b) => b.system(),
            Self::Ws(..) => ActiveSystem::WonderSwan,
        }
    }

    pub(crate) fn core_family(&self) -> CoreFamily {
        self.system().core_family()
    }

    fn state_extension(&self) -> &'static str {
        self.system().state_extension()
    }

    pub(crate) fn gb(&self) -> Option<&GbBackend> {
        match self {
            Self::Gb(b) => Some(b),
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

    pub(crate) fn sega8(&self) -> Option<&Sega8Backend> {
        match self {
            Self::Sega8(b) => Some(b),
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

    pub(crate) fn save_ram_kind(&self) -> SaveRamKind {
        dispatch!(self, save_ram_kind())
    }

    pub(crate) fn has_battery(&self) -> bool {
        self.save_ram_kind().is_battery_backed()
    }

    pub(crate) fn system_ram_len(&self) -> usize {
        dispatch!(self, system_ram_len())
    }

    pub(crate) fn video_ram_len(&self) -> usize {
        dispatch!(self, video_ram_len())
    }

    pub(crate) fn supports_debugger(&self) -> bool {
        dispatch!(self, supports_debugger())
    }

    pub(crate) fn supports_opcode_history(&self) -> bool {
        dispatch!(self, supports_opcode_history())
    }

    pub(crate) fn capabilities(&self) -> CoreCapabilities {
        CoreCapabilities {
            core_family: self.core_family(),
            save_ram_kind: self.save_ram_kind(),
            has_battery: self.has_battery(),
            system_ram_len: self.system_ram_len(),
            video_ram_len: self.video_ram_len(),
            memory_regions: self.memory_regions(),
            input_features: capabilities::InputCapabilities::for_system(self.system()),
            cheat_features: CheatCapabilities::for_system(self.system()),
            supports_save_states: true,
            supports_rewind: true,
            supports_debugger: self.supports_debugger(),
            supports_opcode_history: self.supports_opcode_history(),
        }
    }

    pub(crate) fn memory_regions(&self) -> Vec<MemoryRegionDescriptor> {
        dispatch!(self, memory_regions())
    }

    #[allow(dead_code)]
    pub(crate) fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        dispatch!(self, copy_memory_region(id_or_alias, out))
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

    pub(crate) fn audio_semantic_frame(&self) -> Option<crate::audio_tooling::AudioSemanticFrame> {
        dispatch!(self, audio_semantic_frame())
    }

    #[inline]
    pub(crate) fn step_frame(&mut self) {
        dispatch!(self, step_frame())
    }

    #[inline]
    pub(crate) fn frame_count(&self) -> u64 {
        dispatch!(self, frame_count())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_with_remote_link(
        &mut self,
        link: &mut crate::link::gb::GameBoyRemoteLink<crate::link::transport::TcpLinkTransport>,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut gb.emu)?;
        if gb.emu.game_boy_link_pending_master_response() {
            return Ok(());
        }

        let mut link_error = None;
        gb.emu.step_until_frame_or(|emulator| {
            if let Err(err) = link.poll_emulator(emulator) {
                link_error = Some(err);
                return true;
            }
            if emulator.game_boy_link_pending_master_response() {
                return true;
            }
            false
        });

        if let Some(err) = link_error {
            return Err(err);
        }
        link.poll_emulator(&mut gb.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_wonder_swan_frame_with_remote_link(
        &mut self,
        link: &mut crate::link::ws::WonderSwanRemoteLink<crate::link::transport::TcpLinkTransport>,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Ws(ws) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut ws.emu)?;
        if ws.emu.is_cpu_suspended() {
            return Ok(());
        }

        ws.emu.clear_frame_ready();
        let guard = ws
            .emu
            .cpu_cycles()
            .wrapping_add(u64::from(zeff_ws_core::hardware::constants::CYCLES_PER_FRAME) * 2);
        while !ws.emu.frame_ready() && ws.emu.cpu_cycles() < guard {
            if ws.emu.step_instruction().is_none() && ws.emu.is_cpu_suspended() {
                break;
            }
            link.poll_emulator(&mut ws.emu)?;
        }
        ws.emu.finish_frame();
        link.poll_emulator(&mut ws.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_frame_with_remote_link(
        &mut self,
        link: &mut crate::link::RemoteLink<crate::link::transport::TcpLinkTransport>,
    ) -> Result<(), crate::link::LinkSessionError> {
        match link {
            crate::link::RemoteLink::GameBoy(link) => {
                self.step_game_boy_frame_with_remote_link(link)
            }
            crate::link::RemoteLink::WonderSwan(link) => {
                self.step_wonder_swan_frame_with_remote_link(link)
            }
        }
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

    #[allow(dead_code)]
    pub(crate) fn set_link_peer_present(&mut self, present: bool) -> bool {
        match self {
            Self::Gb(gb) => {
                gb.emu.set_game_boy_link_peer_present(present);
                true
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn game_boy_link_state(
        &self,
    ) -> Option<zeff_gb_core::hardware::bus::GameBoyLinkState> {
        match self {
            Self::Gb(gb) => Some(gb.emu.game_boy_link_state()),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn sync_game_boy_remote_link_state(
        &mut self,
        peer_state: zeff_gb_core::hardware::bus::GameBoyLinkState,
        idle_master_response: Option<u8>,
    ) -> bool {
        match self {
            Self::Gb(gb) => gb.emu.sync_game_boy_remote_link_peer_with_idle_response(
                peer_state,
                idle_master_response,
            ),
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn sync_link_peer(&mut self, peer: &mut Self) -> bool {
        match (self, peer) {
            (Self::Gb(left), Self::Gb(right)) => {
                left.emu.sync_game_boy_link_peer(&mut right.emu);
                true
            }
            (Self::Sega8(left), Self::Sega8(right))
                if left.system() == ActiveSystem::GameGear
                    && right.system() == ActiveSystem::GameGear =>
            {
                left.emu.sync_game_gear_link_peer(&mut right.emu);
                true
            }
            (Self::Ws(left), Self::Ws(right)) => {
                left.emu.sync_wonder_swan_link_peer(&mut right.emu);
                true
            }
            _ => false,
        }
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
