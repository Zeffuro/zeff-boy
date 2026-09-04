use std::path::{Path, PathBuf};

use zeff_emu_common::memory::MemoryRegionDescriptor;
use zeff_emu_common::replay::ReplayJoypadFrame;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::system::CoreFamily;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};

pub(crate) use self::capabilities::{CheatCapabilities, CoreCapabilities, InputCapabilities};
pub(crate) use self::coleco::ColecoBackend;
pub(crate) use self::gb::GbBackend;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::gb::GbTasLoadProvenanceView;
pub(crate) use self::gba::GbaBackend;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::gba::GbaTasLoadProvenanceView;
pub(crate) use self::loader::{BackendLoadConfig, load_backend_from_rom_source};
pub(crate) use self::nes::NesBackend;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::nes::NesTasLoadProvenanceView;
pub(crate) use self::pce::PceBackend;
pub(crate) use self::runtime::BackendRuntimeConfig;
pub(crate) use self::sega8::Sega8Backend;
pub(crate) use self::system::{
    ActiveSystem, ROM_AND_ARCHIVE_EXTENSIONS, ROM_EXTENSIONS, archive_extensions, system_specs,
};
pub(crate) use self::ws::WsBackend;

use crate::emu_core_trait::EmulatorCore;

pub(crate) mod capabilities;
pub(crate) mod cheats;
pub(crate) mod coleco;
pub(crate) mod firmware;
pub(crate) mod gb;
pub(crate) mod gba;
mod link;
pub(crate) mod loader;
pub(crate) mod nes;
pub(crate) mod paths;
pub(crate) mod pce;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd_archive;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd_chd;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd_file;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd_overlay;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd_rar;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pce_cd_zip;
mod pce_display;
pub(crate) mod pce_profiles;
#[cfg(all(feature = "profile-cores", not(target_arch = "wasm32")))]
pub(crate) use pce_cd_archive::profile_cache_load as profile_pce_cd_cache;
#[cfg(feature = "profile-cores")]
pub(crate) use pce_display::profile_projection as profile_pce_projection;
mod pce_palette;
pub(crate) mod runtime;
pub(crate) mod sega8;
pub(crate) mod system;
pub(crate) mod ws;

pub(crate) enum EmuBackend {
    Gb(Box<GbBackend>),
    Gba(Box<GbaBackend>),
    Nes(Box<NesBackend>),
    Coleco(Box<ColecoBackend>),
    Pce(Box<PceBackend>),
    Sega8(Box<Sega8Backend>),
    Ws(Box<WsBackend>),
}

pub(crate) enum DetachedFrameBackend {
    Gba {
        emu: Box<zeff_gba_core::emulator::Emulator>,
        #[cfg(test)]
        force_operational_failure: bool,
    },
    Sega8 {
        emu: Box<zeff_sega8_core::emulator::Emulator>,
        #[cfg(test)]
        force_operational_failure: bool,
    },
}

impl DetachedFrameBackend {
    pub(crate) fn frame_count(&self) -> u64 {
        match self {
            Self::Gba { emu, .. } => FrameLifecycle::frame_count(emu.as_ref()),
            Self::Sega8 { emu, .. } => FrameLifecycle::frame_count(emu.as_ref()),
        }
    }

    pub(crate) fn step_frames(&mut self, frames: usize) -> bool {
        #[cfg(test)]
        if matches!(
            self,
            Self::Gba {
                force_operational_failure: true,
                ..
            } | Self::Sega8 {
                force_operational_failure: true,
                ..
            }
        ) {
            return false;
        }
        for _ in 0..frames {
            let before = self.frame_count();
            match self {
                Self::Gba { emu, .. } => FrameLifecycle::step_frame(emu.as_mut()),
                Self::Sega8 { emu, .. } => FrameLifecycle::step_frame(emu.as_mut()),
            }
            let after = self.frame_count();
            if before.checked_add(1) != Some(after) {
                return false;
            }
        }
        true
    }

    pub(crate) fn disable_audio_output(&mut self) {
        match self {
            Self::Gba { emu, .. } => emu.set_apu_sample_generation_enabled(false),
            Self::Sega8 { emu, .. } => emu.set_apu_sample_generation_enabled(false),
        }
    }

    pub(crate) fn framebuffer(&self) -> &[u8] {
        match self {
            Self::Gba { emu, .. } => emu.framebuffer(),
            Self::Sega8 { emu, .. } => emu.framebuffer(),
        }
    }

    #[cfg(test)]
    pub(crate) fn force_operational_failure_for_test(&mut self) {
        match self {
            Self::Gba {
                force_operational_failure,
                ..
            }
            | Self::Sega8 {
                force_operational_failure,
                ..
            } => *force_operational_failure = true,
        }
    }
}

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            EmuBackend::Gb(b) => b.$method($($arg),*),
            EmuBackend::Gba(b) => b.$method($($arg),*),
            EmuBackend::Nes(b) => b.$method($($arg),*),
            EmuBackend::Coleco(b) => b.$method($($arg),*),
            EmuBackend::Pce(b) => b.$method($($arg),*),
            EmuBackend::Sega8(b) => b.$method($($arg),*),
            EmuBackend::Ws(b) => b.$method($($arg),*),
        }
    };
}

mod state_io;
pub(crate) use state_io::canonicalize_state_bytes_for_replay_hash;

impl EmuBackend {
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    pub(crate) fn nes_tas_load_provenance(&self) -> Option<NesTasLoadProvenanceView<'_>> {
        self.nes()?.tas_load_provenance()
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    pub(crate) fn gb_tas_load_provenance(&self) -> Option<GbTasLoadProvenanceView<'_>> {
        self.gb()?.tas_load_provenance()
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    pub(crate) fn gba_tas_load_provenance(&self) -> Option<GbaTasLoadProvenanceView<'_>> {
        self.gba()?.tas_load_provenance()
    }

    pub(crate) fn nes_has_standard_controller_topology(&self) -> Option<bool> {
        match self {
            Self::Nes(backend) => Some(backend.emu.has_standard_controller_topology()),
            _ => None,
        }
    }

    pub(crate) fn nes_has_standard_or_zapper_controller_topology(&self) -> Option<bool> {
        self.nes()
            .map(|backend| backend.emu.has_standard_or_zapper_controller_topology())
    }

    pub(crate) fn supports_detached_speculation(&self) -> bool {
        matches!(self, Self::Gba(_))
            || matches!(self, Self::Sega8(_)) && self.system() == ActiveSystem::MasterSystem
    }

    pub(crate) fn fork_detached_for_speculation(&self) -> Option<DetachedFrameBackend> {
        match self {
            Self::Gba(backend) => Some(DetachedFrameBackend::Gba {
                emu: Box::new(backend.emu.clone()),
                #[cfg(test)]
                force_operational_failure: false,
            }),
            Self::Sega8(backend) if backend.system() == ActiveSystem::MasterSystem => {
                Some(DetachedFrameBackend::Sega8 {
                    emu: Box::new(backend.emu.clone()),
                    #[cfg(test)]
                    force_operational_failure: false,
                })
            }
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_gb(emu: zeff_gb_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Gb(Box::new(GbBackend::new(emu, rom_path)))
    }

    #[allow(dead_code)]
    pub(crate) fn from_gba(emu: zeff_gba_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Gba(Box::new(GbaBackend::new(emu, rom_path)))
    }

    #[allow(dead_code)]
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

    pub(crate) fn from_gba_with_tas_load_provenance(
        emu: zeff_gba_core::emulator::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        provenance: crate::emu_backend::gba::GbaTasLoadProvenance,
    ) -> Self {
        Self::Gba(Box::new(GbaBackend::with_tas_load_provenance(
            emu,
            rom_path,
            source_path,
            provenance,
        )))
    }

    #[allow(dead_code)]
    pub(crate) fn from_nes(emu: zeff_nes_core::emulator::Emulator, rom_path: PathBuf) -> Self {
        Self::Nes(Box::new(NesBackend::new(emu, rom_path)))
    }

    pub(crate) fn from_coleco(
        emu: zeff_coleco_core::Emulator,
        rom_path: PathBuf,
        rom_hash: [u8; 32],
    ) -> Self {
        Self::Coleco(Box::new(ColecoBackend::new(emu, rom_path, rom_hash)))
    }

    pub(crate) fn from_coleco_with_source(
        emu: zeff_coleco_core::Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
    ) -> Self {
        Self::Coleco(Box::new(ColecoBackend::with_source_path(
            emu,
            rom_path,
            source_path,
            rom_hash,
        )))
    }

    pub(crate) fn from_pce(backend: PceBackend) -> Self {
        Self::Pce(Box::new(backend))
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
            Self::Coleco(..) => ActiveSystem::Coleco,
            Self::Pce(..) => ActiveSystem::Pce,
            Self::Sega8(b) => b.system(),
            Self::Ws(..) => ActiveSystem::WonderSwan,
        }
    }

    pub(crate) fn nominal_frame_duration_ns(&self) -> u64 {
        match self {
            Self::Nes(backend) => backend.nominal_frame_duration_ns(),
            Self::Sega8(backend) => backend.nominal_frame_duration_ns(),
            _ => self.system().frame_duration_ns(),
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

    pub(crate) fn coleco(&self) -> Option<&ColecoBackend> {
        match self {
            Self::Coleco(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn pce(&self) -> Option<&PceBackend> {
        match self {
            Self::Pce(b) => Some(b),
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

    pub(crate) fn debug_suspend(&mut self) {
        if !self.supports_execution_controls() {
            return;
        }
        dispatch!(self, debug_suspend())
    }

    pub(crate) fn rom_path(&self) -> &Path {
        dispatch!(self, rom_path())
    }

    pub(crate) fn source_path(&self) -> &Path {
        dispatch!(self, source_path())
    }

    pub(crate) fn replay_metadata(&self) -> zeff_emu_common::replay::ReplayMetadata {
        let rom_sha256 = match self {
            Self::Pce(pce)
                if pce
                    .tas_load_provenance()
                    .is_some_and(|view| view.load.direct_pce_cd) =>
            {
                pce.normalized_disc_hash()
                    .unwrap_or_else(|| self.rom_hash())
            }
            _ => self.rom_hash(),
        };
        zeff_emu_common::replay::ReplayMetadata {
            system: Some(self.system().code().to_owned()),
            core_family: Some(format!("{:?}", self.core_family())),
            rom_sha256: Some(rom_sha256),
            firmware: dispatch!(self, firmware_manifests()).to_vec(),
            events: Vec::new(),
            cheat_sha256: None,
            final_state_sha256: None,
            game_boy_link_start_state: None,
            game_boy_link_coordinator_start_state: None,
            game_boy_link_start_tick: None,
            wonder_swan_link_start_tick: None,
            checkpoints: Vec::new(),
        }
    }

    pub(crate) fn set_firmware_manifests(
        &mut self,
        firmware_manifests: Vec<zeff_emu_common::replay::ReplayFirmwareManifest>,
    ) {
        dispatch!(self, set_firmware_manifests(firmware_manifests))
    }

    pub(crate) fn rom_hash(&self) -> [u8; 32] {
        dispatch!(self, rom_hash())
    }

    pub(crate) fn recovery_discriminator(&self) -> String {
        format!(
            "zeff-{}-native-{}",
            self.system().storage_subdir(),
            self.state_extension()
        )
    }

    pub(crate) fn battery_components(&self) -> Vec<(&'static str, Vec<u8>)> {
        match self {
            Self::Gb(backend) => backend.battery_components(),
            Self::Gba(backend) => backend.battery_components(),
            Self::Nes(backend) => backend.battery_components(),
            Self::Coleco(backend) => backend.battery_components(),
            Self::Pce(backend) => backend.battery_components(),
            Self::Sega8(backend) => backend.battery_components(),
            Self::Ws(backend) => backend.battery_components(),
        }
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    pub(crate) fn battery_component_hash(&self) -> [u8; 32] {
        let components = self.battery_components();
        let borrowed = components
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice()))
            .collect::<Vec<_>>();
        crate::save_paths::recovery_state::canonical_battery_component_hash(&borrowed)
    }

    pub(crate) fn battery_generation_receipt(
        &self,
    ) -> anyhow::Result<crate::save_paths::recovery_state::BatteryPublicationReceipt> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Self::Gb(backend) = self
            && let Some(receipt) = backend.persisted_rtc_battery_receipt()?
        {
            return Ok(receipt);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Self::Gba(backend) = self
            && let Some(receipt) = backend.persisted_rtc_battery_receipt()?
        {
            return Ok(receipt);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Self::Ws(backend) = self
            && let Some(receipt) = backend.persisted_rtc_battery_receipt()?
        {
            return Ok(receipt);
        }
        let components = self.battery_components();
        let borrowed = components
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice()))
            .collect::<Vec<_>>();
        Ok(
            crate::save_paths::recovery_state::BatteryPublicationReceipt::from_components(
                &borrowed,
            ),
        )
    }

    pub(crate) fn pce_controller_profile_hash(&self) -> Option<[u8; 32]> {
        let Self::Pce(backend) = self else {
            return None;
        };
        Some(backend.controller_profile_hash())
    }

    pub(crate) fn save_ram_kind(&self) -> SaveRamKind {
        dispatch!(self, save_ram_kind())
    }

    pub(crate) fn has_battery(&self) -> bool {
        self.save_ram_kind().is_battery_backed()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn nes_tas_battery_bytes(&self) -> Option<Vec<u8>> {
        let Self::Nes(backend) = self else {
            return None;
        };
        backend.tas_battery_bytes()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn nes_tas_battery_baseline(
        &self,
    ) -> anyhow::Result<crate::save_paths::SaveTargetBaseline> {
        let Self::Nes(backend) = self else {
            anyhow::bail!("TAS battery publication currently supports only NES");
        };
        backend.tas_battery_baseline()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_nes_tas_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(String, crate::save_paths::SavePublicationOutcome)> {
        let Self::Nes(backend) = self else {
            return None;
        };
        backend.publish_tas_battery_if_unchanged(expected)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn gb_tas_battery_bytes(&self) -> Option<Vec<u8>> {
        let Self::Gb(backend) = self else {
            return None;
        };
        backend.tas_battery_bytes()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn gb_tas_rtc_battery_bytes(&self) -> Option<Vec<u8>> {
        crate::emu_backend::loader::gb_rtc_complete_persistence_bytes(self).ok()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn gb_tas_battery_baseline(
        &self,
    ) -> anyhow::Result<crate::save_paths::SaveTargetBaseline> {
        let Self::Gb(backend) = self else {
            anyhow::bail!("TAS battery publication requires a Game Boy backend");
        };
        backend.tas_battery_baseline()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_gb_tas_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(String, crate::save_paths::SavePublicationOutcome)> {
        let Self::Gb(backend) = self else {
            return None;
        };
        backend.publish_tas_battery_if_unchanged(expected)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_gb_tas_rtc_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(
        String,
        crate::save_paths::SavePublicationOutcome,
        crate::save_paths::recovery_state::BatteryPublicationReceipt,
    )> {
        let Self::Gb(backend) = self else {
            return None;
        };
        backend.publish_tas_rtc_battery_if_unchanged(expected)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn gba_tas_battery_component(
        &self,
    ) -> anyhow::Result<Option<(crate::emu_thread::TasGbaPersistenceKind, Vec<u8>)>> {
        let Self::Gba(backend) = self else {
            return Ok(None);
        };
        backend.tas_battery_component()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn gba_tas_rtc_battery_bytes(&self) -> Option<Vec<u8>> {
        let Self::Gba(backend) = self else {
            return None;
        };
        backend.tas_rtc_battery_bytes()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn gba_tas_battery_baseline(
        &self,
    ) -> anyhow::Result<crate::save_paths::SaveTargetBaseline> {
        let Self::Gba(backend) = self else {
            anyhow::bail!("TAS battery publication requires a GBA backend");
        };
        backend.tas_battery_baseline()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_gba_tas_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> anyhow::Result<Option<(String, crate::save_paths::SavePublicationOutcome)>> {
        let Self::Gba(backend) = self else {
            return Ok(None);
        };
        backend.publish_tas_battery_if_unchanged(expected)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_gba_tas_rtc_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(
        String,
        crate::save_paths::SavePublicationOutcome,
        crate::save_paths::recovery_state::BatteryPublicationReceipt,
    )> {
        let Self::Gba(backend) = self else {
            return None;
        };
        backend.publish_tas_rtc_battery_if_unchanged(expected)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn game_gear_tas_battery_bytes(&self) -> Option<Vec<u8>> {
        let Self::Sega8(backend) = self else {
            return None;
        };
        backend.game_gear_tas_battery_bytes()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn game_gear_tas_battery_baseline(
        &self,
    ) -> anyhow::Result<crate::save_paths::SaveTargetBaseline> {
        let Self::Sega8(backend) = self else {
            anyhow::bail!("Game Gear TAS battery baseline requires a Sega 8-bit backend");
        };
        backend.game_gear_tas_battery_baseline()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_game_gear_tas_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(String, crate::save_paths::SavePublicationOutcome)> {
        let Self::Sega8(backend) = self else {
            return None;
        };
        backend.publish_game_gear_tas_battery_if_unchanged(expected)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn ws_tas_battery_bytes(&self) -> Option<Vec<u8>> {
        let Self::Ws(backend) = self else {
            return None;
        };
        backend.tas_battery_bytes()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn ws_tas_rtc_battery_bytes(&self) -> Option<Vec<u8>> {
        let Self::Ws(backend) = self else {
            return None;
        };
        backend.tas_rtc_battery_bytes()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn ws_tas_battery_save_kind(
        &self,
    ) -> Option<zeff_ws_core::hardware::cartridge::SaveKind> {
        let Self::Ws(backend) = self else {
            return None;
        };
        Some(backend.tas_battery_save_kind())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn ws_tas_battery_baseline(
        &self,
    ) -> anyhow::Result<crate::save_paths::SaveTargetBaseline> {
        let Self::Ws(backend) = self else {
            anyhow::bail!("TAS battery publication requires a WonderSwan backend");
        };
        backend.tas_battery_baseline()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_ws_tas_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(String, crate::save_paths::SavePublicationOutcome)> {
        let Self::Ws(backend) = self else {
            return None;
        };
        backend.publish_tas_battery_if_unchanged(expected)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_ws_tas_rtc_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(
        String,
        crate::save_paths::SavePublicationOutcome,
        crate::save_paths::recovery_state::BatteryPublicationReceipt,
    )> {
        let Self::Ws(backend) = self else {
            return None;
        };
        backend.publish_tas_rtc_battery_if_unchanged(expected)
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

    pub(crate) fn supports_execution_controls(&self) -> bool {
        dispatch!(self, supports_execution_controls())
    }

    pub(crate) fn supports_symbol_loading(&self) -> bool {
        dispatch!(self, supports_symbol_loading())
    }

    pub(crate) fn supports_opcode_history(&self) -> bool {
        dispatch!(self, supports_opcode_history())
    }

    pub(crate) fn supports_save_states(&self) -> bool {
        dispatch!(self, supports_save_states())
    }

    pub(crate) fn supports_state_capture(&self) -> bool {
        dispatch!(self, supports_state_capture())
    }

    pub(crate) fn supports_rewind(&self) -> bool {
        dispatch!(self, supports_rewind())
    }

    pub(crate) fn supports_replay(&self) -> bool {
        dispatch!(self, supports_replay())
    }

    pub(crate) fn supports_audio(&self) -> bool {
        dispatch!(self, supports_audio())
    }

    pub(crate) fn supports_cheats(&self) -> bool {
        dispatch!(self, supports_cheats())
    }

    pub(crate) fn supports_guest_calls(&self) -> bool {
        dispatch!(self, supports_guest_calls())
    }

    pub(crate) fn take_runtime_fault(&mut self) -> Option<String> {
        dispatch!(self, take_runtime_fault())
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

    pub(crate) fn is_gba_tilt(&self) -> bool {
        matches!(
            self,
            Self::Gba(gba)
                if gba.emu.sensor_kind()
                    == zeff_gba_core::hardware::cartridge::SensorKind::Tilt
        )
    }

    #[inline]
    pub(crate) fn is_pocket_camera(&self) -> bool {
        dispatch!(self, is_pocket_camera())
    }

    pub(crate) fn audio_semantic_frame(&self) -> Option<crate::audio_tooling::AudioSemanticFrame> {
        if !self.supports_audio() {
            return None;
        }
        let frame = dispatch!(self, audio_semantic_frame());
        if let (Some(topology), Some(frame)) = (self.audio_topology(), frame.as_ref()) {
            crate::audio_tooling::debug_assert_frame_matches_topology(topology, frame);
        }
        frame
    }

    pub(crate) fn audio_topology(&self) -> Option<crate::audio_tooling::AudioTopology> {
        self.supports_audio()
            .then(|| dispatch!(self, audio_topology()))
            .flatten()
    }

    #[inline]
    pub(crate) fn step_frame(&mut self) {
        FrameLifecycle::step_frame(self)
    }

    #[inline]
    pub(crate) fn frame_count(&self) -> u64 {
        FrameLifecycle::frame_count(self)
    }

    #[inline]
    pub(crate) fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        if self.supports_audio() {
            dispatch!(self, drain_audio_samples_into(buf))
        }
    }

    pub(crate) fn set_sample_rate(&mut self, rate: u32) {
        if self.supports_audio() {
            dispatch!(self, set_sample_rate(rate))
        }
    }

    pub(crate) fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        if self.supports_audio() {
            dispatch!(self, set_apu_sample_generation_enabled(enabled))
        }
    }

    pub(crate) fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        if self.supports_audio() {
            dispatch!(self, set_apu_channel_mutes(mutes))
        }
    }

    #[inline]
    pub(crate) fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input(buttons_pressed, dpad_pressed))
    }

    pub(crate) fn set_pce_mouse_state(
        &mut self,
        mode: zeff_pce_core::hardware::PceControllerMode,
        delta_x: i16,
        delta_y: i16,
        buttons_pressed: u8,
    ) {
        dispatch!(
            self,
            set_pce_mouse_state(mode, delta_x, delta_y, buttons_pressed)
        )
    }

    pub(crate) fn set_pce_memory_base_mode(
        &mut self,
        mode: zeff_pce_core::hardware::PceMemoryBaseMode,
    ) {
        dispatch!(self, set_pce_memory_base_mode(mode))
    }

    pub(crate) fn apply_replay_input(&mut self, frame: &ReplayJoypadFrame) {
        self.set_input(frame.buttons, frame.dpad);
        self.set_input_p2(frame.buttons_p2, frame.dpad_p2);
        self.set_input_p3(frame.buttons_p3, frame.dpad_p3);
        self.set_input_p4(frame.buttons_p4, frame.dpad_p4);
        self.set_input_p5(frame.buttons_p5, frame.dpad_p5);
        self.set_zapper_state(
            frame.zapper.enabled,
            frame.zapper.trigger,
            frame.zapper.hit,
            frame.zapper.screen_pos,
        );
        self.set_replay_host_tilt(frame.host_tilt);
        if let Some(camera_frame) = frame.camera_frame.as_deref() {
            self.set_replay_camera_frame(camera_frame);
        }
    }

    pub(crate) fn apply_coleco_tas_input(
        &mut self,
        controllers: [crate::tas_project::TasColecoControllerInput; 2],
    ) -> anyhow::Result<()> {
        let Self::Coleco(backend) = self else {
            anyhow::bail!("ColecoVision TAS input requires a ColecoVision backend");
        };
        backend.set_tas_controllers(controllers);
        Ok(())
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

    pub(crate) fn set_replay_host_tilt(&mut self, host_tilt: (f32, f32)) {
        match self {
            Self::Gb(gb) => gb.emu.set_mbc7_host_tilt(host_tilt.0, host_tilt.1),
            Self::Gba(gba) => {
                gba.emu.set_tilt_input(host_tilt.0, host_tilt.1);
            }
            _ => {}
        }
    }

    pub(crate) fn set_replay_camera_frame(&mut self, camera_frame: &[u8]) {
        if let Self::Gb(gb) = self {
            gb.emu.set_camera_host_frame(camera_frame);
        }
    }

    pub(crate) fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input_p2(buttons_pressed, dpad_pressed))
    }

    #[inline]
    pub(crate) fn set_input_p3(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input_p3(buttons_pressed, dpad_pressed))
    }

    #[inline]
    pub(crate) fn set_input_p4(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input_p4(buttons_pressed, dpad_pressed))
    }

    #[inline]
    pub(crate) fn set_input_p5(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        dispatch!(self, set_input_p5(buttons_pressed, dpad_pressed))
    }

    pub(crate) fn set_fds_disk_side(&mut self, side: u8) -> anyhow::Result<()> {
        match self {
            Self::Nes(nes) => nes.set_fds_disk_side(side),
            _ => anyhow::bail!("FDS disk side selection is only available for NES/FDS content"),
        }
    }

    #[cfg(test)]
    pub(crate) fn fds_disk_side(&self) -> Option<u8> {
        match self {
            Self::Nes(nes) => nes.fds_disk_side(),
            _ => None,
        }
    }

    pub(crate) fn media_slot_snapshot(&self) -> Option<zeff_emu_common::media::MediaSlotSnapshot> {
        match self {
            Self::Nes(nes) => nes.media_slot_snapshot(),
            _ => None,
        }
    }

    pub(crate) fn apply_media_event(
        &mut self,
        event: &zeff_emu_common::media::MediaEvent,
    ) -> anyhow::Result<()> {
        match self {
            Self::Nes(nes) => nes.apply_media_event(event),
            _ => anyhow::bail!("current system has no removable media slot"),
        }
    }

    pub(crate) fn game_boy_serial_device(
        &self,
    ) -> Option<zeff_gb_core::hardware::GameBoySerialDevice> {
        match self {
            Self::Gb(gb) => Some(gb.emu.game_boy_serial_device()),
            _ => None,
        }
    }

    pub(crate) fn set_game_boy_serial_device(
        &mut self,
        device: zeff_gb_core::hardware::GameBoySerialDevice,
    ) -> bool {
        match self {
            Self::Gb(gb) => {
                gb.emu.set_game_boy_serial_device(device);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn queue_bardigun_barcode_scan(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        match self {
            Self::Gb(gb) => gb.emu.queue_bardigun_barcode_scan(bytes),
            _ => anyhow::bail!("current system has no Game Boy serial device"),
        }
    }

    pub(crate) fn trigger_barcode_boy_scan(&mut self, digits: &str) -> anyhow::Result<()> {
        match self {
            Self::Gb(gb) => gb.emu.trigger_barcode_boy_scan(digits),
            _ => anyhow::bail!("current system has no Game Boy serial device"),
        }
    }

    pub(crate) fn take_game_boy_printer_jobs(
        &mut self,
    ) -> Vec<zeff_gb_core::hardware::GameBoyPrinterJob> {
        match self {
            Self::Gb(gb) => gb.emu.take_printer_jobs(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn discard_game_boy_printer_jobs(&mut self) {
        if let Self::Gb(gb) = self {
            gb.emu.clear_printer_jobs();
        }
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

    pub(crate) fn game_boy_link_replay_state(
        &self,
    ) -> Option<zeff_emu_common::replay::ReplayGameBoyLinkState> {
        match self {
            Self::Gb(gb) => Some(gb.emu.game_boy_link_replay_state()),
            _ => None,
        }
    }

    pub(crate) fn restore_game_boy_link_replay_state(
        &mut self,
        state: zeff_emu_common::replay::ReplayGameBoyLinkState,
    ) -> bool {
        match self {
            Self::Gb(gb) => gb.emu.restore_game_boy_link_replay_state(state),
            _ => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn apply_game_boy_link_reply(
        &mut self,
        reply: zeff_emu_common::replay::ReplayGameBoyLinkReply,
    ) -> bool {
        match self {
            Self::Gb(gb) => {
                gb.emu
                    .apply_game_boy_link_reply(zeff_gb_core::hardware::bus::GameBoyLinkReply {
                        out_byte: reply.out_byte,
                        passive: reply.passive,
                        serial_generation: reply.serial_generation,
                    })
            }
            _ => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn preview_game_boy_link_peer(
        &self,
        peer: &Self,
    ) -> Option<zeff_gb_core::hardware::bus::GameBoyLinkExchangePreview> {
        match (self, peer) {
            (Self::Gb(left), Self::Gb(right)) => {
                Some(left.emu.preview_game_boy_link_peer(&right.emu))
            }
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn try_prepare_game_boy_link_peer(
        &mut self,
        peer: &mut Self,
    ) -> Result<
        zeff_gb_core::hardware::bus::GameBoyLinkPreparedExchange,
        zeff_gb_core::hardware::bus::GameBoyLinkExchangeError,
    > {
        match (self, peer) {
            (Self::Gb(left), Self::Gb(right)) => {
                left.emu.try_prepare_game_boy_link_peer(&mut right.emu)
            }
            _ => unreachable!("paired Game Boy exchange requires two GB backends"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn try_apply_prepared_game_boy_link_reply(
        &mut self,
        transfer: zeff_gb_core::hardware::bus::GameBoyLinkPreparedTransfer,
    ) -> Result<
        zeff_gb_core::hardware::bus::GameBoyLinkTransferExchange,
        zeff_gb_core::hardware::bus::GameBoyLinkExchangeError,
    > {
        match self {
            Self::Gb(gb) => gb.emu.try_apply_prepared_game_boy_link_reply(transfer),
            _ => unreachable!("paired Game Boy reply requires a GB backend"),
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

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn acknowledge_battery_commit(&mut self, snapshot_still_matches: bool) {
        if let Self::Pce(backend) = self {
            backend.acknowledge_battery_commit(snapshot_still_matches);
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.is_suspended()
    }
}

impl MachineTiming for EmuBackend {
    #[inline]
    fn timing_snapshot(&self) -> TimingSnapshot {
        dispatch!(self, timing_snapshot())
    }
}

impl Reset for EmuBackend {
    #[inline]
    fn reset(&mut self) {
        dispatch!(self, reset())
    }
}

impl FrameLifecycle for EmuBackend {
    #[inline]
    fn step_frame(&mut self) {
        dispatch!(self, step_frame())
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        dispatch!(self, frame_count())
    }
}

#[cfg(test)]
mod tests;
