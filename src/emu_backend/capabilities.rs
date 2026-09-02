use zeff_emu_common::memory::MemoryRegionDescriptor;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::system::CoreFamily;

use crate::input::HostButton;

use super::ActiveSystem;

impl super::EmuBackend {
    pub(super) fn with_coleco_tas_load_provenance(
        self,
        provenance: super::coleco::ColecoTasLoadProvenanceSeed,
    ) -> Self {
        match self {
            Self::Coleco(backend) => Self::Coleco(Box::new(
                (*backend).with_tas_load_provenance(provenance.finish()),
            )),
            backend => backend,
        }
    }

    pub(super) fn with_sms_tas_load_provenance(
        self,
        provenance: super::sega8::tas_provenance::SmsTasLoadProvenance,
    ) -> Self {
        match self {
            Self::Sega8(backend) => Self::Sega8(Box::new(
                (*backend).with_sms_tas_load_provenance(provenance),
            )),
            backend => backend,
        }
    }

    pub(super) fn with_game_gear_tas_load_provenance(
        self,
        provenance: super::sega8::GameGearTasLoadProvenance,
    ) -> Self {
        match self {
            Self::Sega8(backend) => Self::Sega8(Box::new(
                (*backend).with_game_gear_tas_load_provenance(provenance),
            )),
            backend => backend,
        }
    }

    pub(super) fn with_sg1000_tas_load_provenance(
        self,
        provenance: super::sega8::Sg1000TasLoadProvenance,
    ) -> Self {
        match self {
            Self::Sega8(backend) => Self::Sega8(Box::new(
                (*backend).with_sg1000_tas_load_provenance(provenance),
            )),
            backend => backend,
        }
    }

    pub(crate) fn tas_source_media_identity(&self) -> Option<TasSourceMediaIdentity> {
        match self {
            Self::Coleco(backend) => backend.tas_source_media_identity(),
            Self::Gb(backend) => backend.tas_source_media_identity(),
            Self::Gba(backend) => backend.tas_source_media_identity(),
            Self::Pce(backend) => backend.tas_source_media_identity(),
            Self::Ws(backend) => backend.tas_source_media_identity(),
            Self::Sega8(backend) => backend
                .sms_tas_source_media_identity()
                .or_else(|| backend.game_gear_tas_source_media_identity())
                .or_else(|| backend.sg1000_tas_source_media_identity()),
            _ => None,
        }
    }

    pub(crate) fn capabilities(&self) -> CoreCapabilities {
        CoreCapabilities {
            core_family: self.core_family(),
            save_ram_kind: self.save_ram_kind(),
            has_battery: self.has_battery(),
            system_ram_len: self.system_ram_len(),
            video_ram_len: self.video_ram_len(),
            memory_regions: self.memory_regions(),
            input_features: InputCapabilities::for_system(self.system()),
            cheat_features: CheatCapabilities::for_system(self.system()),
            supports_save_states: self.supports_save_states(),
            supports_state_capture: self.supports_state_capture(),
            supports_rewind: self.supports_rewind(),
            supports_replay: self.supports_replay(),
            supports_audio: self.supports_audio(),
            supports_cheats: self.supports_cheats(),
            supports_guest_calls: self.supports_guest_calls(),
            supports_debugger: self.supports_debugger(),
            supports_execution_controls: self.supports_execution_controls(),
            supports_opcode_history: self.supports_opcode_history(),
            tas_execution_primitives: TasCoreCapabilityProbe::for_backend(self),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoreCapabilities {
    pub(crate) core_family: CoreFamily,
    pub(crate) save_ram_kind: SaveRamKind,
    pub(crate) has_battery: bool,
    pub(crate) system_ram_len: usize,
    pub(crate) video_ram_len: usize,
    pub(crate) memory_regions: Vec<MemoryRegionDescriptor>,
    pub(crate) input_features: InputCapabilities,
    pub(crate) cheat_features: CheatCapabilities,
    pub(crate) supports_save_states: bool,
    pub(crate) supports_state_capture: bool,
    pub(crate) supports_rewind: bool,
    pub(crate) supports_replay: bool,
    pub(crate) supports_audio: bool,
    pub(crate) supports_cheats: bool,
    pub(crate) supports_guest_calls: bool,
    pub(crate) supports_debugger: bool,
    pub(crate) supports_execution_controls: bool,
    pub(crate) supports_opcode_history: bool,
    pub(crate) tas_execution_primitives: TasCoreCapabilityProbe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TasCoreCapabilityProbe {
    pub(crate) system_identity_observed: bool,
    pub(crate) source_media_identity: Option<TasSourceMediaIdentity>,
    pub(crate) source_media_identity_observed: bool,
    pub(crate) effective_media_identity_observed: bool,
    pub(crate) firmware_identity_observed: bool,
    pub(crate) direct_runtime_profile_requirements_match: bool,
    pub(crate) supports_state_restore: bool,
    pub(crate) persistent_state: TasPersistentStateIdentity,
    pub(crate) input_model: TasInputModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TasSourceMediaIdentity {
    pub(crate) sha256: [u8; 32],
    pub(crate) byte_len: usize,
}

impl TasSourceMediaIdentity {
    pub(crate) const fn new(sha256: [u8; 32], byte_len: usize) -> Self {
        Self { sha256, byte_len }
    }
}

impl TasCoreCapabilityProbe {
    pub(super) fn for_backend(backend: &super::EmuBackend) -> Self {
        let metadata = backend.replay_metadata();
        let core_family = format!("{:?}", backend.core_family());
        let source_media_identity = backend.tas_source_media_identity();
        Self {
            system_identity_observed: metadata.system.as_deref() == Some(backend.system().code())
                && metadata.core_family.as_deref() == Some(core_family.as_str()),
            source_media_identity,
            source_media_identity_observed: source_media_identity.is_some(),
            effective_media_identity_observed: metadata.rom_sha256.is_some(),
            firmware_identity_observed: !metadata.firmware.is_empty(),
            direct_runtime_profile_requirements_match: direct_runtime_profile_requirements_match(
                backend,
            ),
            supports_state_restore: backend.supports_state_capture(),
            persistent_state: TasPersistentStateIdentity::from_save_ram_kind(
                backend.save_ram_kind(),
            ),
            input_model: TasInputModel::for_system(backend.system()),
        }
    }
}

fn direct_runtime_profile_requirements_match(backend: &super::EmuBackend) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        backend.system() == ActiveSystem::Coleco
            && super::loader::validate_direct_coleco_tas_runtime(backend, false).is_ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = backend;
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasPersistentStateIdentity {
    Absent,
    VolatileOnly { size: usize },
    ExternalPersistent { size: usize },
    Unknown { size: usize },
}

impl TasPersistentStateIdentity {
    pub(crate) const fn from_save_ram_kind(save_ram_kind: SaveRamKind) -> Self {
        match save_ram_kind {
            SaveRamKind::None => Self::Absent,
            SaveRamKind::KnownVolatile { size } => Self::VolatileOnly { size },
            SaveRamKind::KnownBatteryBacked { size } => Self::ExternalPersistent { size },
            SaveRamKind::MapperRamUnknown { size } => Self::Unknown { size },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TasInputModel {
    StandardDigitalPads { max_players: u8 },
    GameBoyJoypad,
    ColecoStandardController { max_players: u8 },
    WonderSwanDirectButtons,
}

impl TasInputModel {
    pub(crate) const fn for_system(system: ActiveSystem) -> Self {
        match system {
            ActiveSystem::GameBoy => Self::GameBoyJoypad,
            ActiveSystem::Coleco => Self::ColecoStandardController { max_players: 2 },
            ActiveSystem::WonderSwan => Self::WonderSwanDirectButtons,
            ActiveSystem::GameBoyAdvance => Self::StandardDigitalPads { max_players: 1 },
            ActiveSystem::Nes
            | ActiveSystem::MasterSystem
            | ActiveSystem::Sg1000
            | ActiveSystem::GameGear => Self::StandardDigitalPads { max_players: 2 },
            ActiveSystem::Pce => Self::StandardDigitalPads { max_players: 5 },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InputCapabilities {
    pub(crate) buttons: &'static [HostButton],
    pub(crate) max_players: u8,
    pub(crate) supports_lightgun: bool,
    pub(crate) supports_wonderswan_direct_buttons: bool,
}

impl InputCapabilities {
    pub(crate) fn for_system(system: ActiveSystem) -> Self {
        match system {
            ActiveSystem::GameBoyAdvance => Self {
                buttons: HostButton::WITH_SHOULDERS,
                max_players: 1,
                supports_lightgun: false,
                supports_wonderswan_direct_buttons: false,
            },
            ActiveSystem::Nes => Self {
                buttons: HostButton::STANDARD,
                max_players: 2,
                supports_lightgun: true,
                supports_wonderswan_direct_buttons: false,
            },
            ActiveSystem::Coleco => Self {
                buttons: HostButton::STANDARD,
                max_players: 2,
                supports_lightgun: false,
                supports_wonderswan_direct_buttons: false,
            },
            ActiveSystem::MasterSystem | ActiveSystem::Sg1000 => Self {
                buttons: HostButton::STANDARD,
                max_players: 2,
                supports_lightgun: false,
                supports_wonderswan_direct_buttons: false,
            },
            ActiveSystem::WonderSwan => Self {
                buttons: HostButton::STANDARD,
                max_players: 1,
                supports_lightgun: false,
                supports_wonderswan_direct_buttons: true,
            },
            ActiveSystem::Pce => Self {
                buttons: HostButton::WITH_SIX_BUTTONS,
                max_players: 5,
                supports_lightgun: false,
                supports_wonderswan_direct_buttons: false,
            },
            ActiveSystem::GameBoy | ActiveSystem::GameGear => Self {
                buttons: HostButton::STANDARD,
                max_players: 1,
                supports_lightgun: false,
                supports_wonderswan_direct_buttons: false,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheatCapabilities {
    pub(crate) supports_user_cheats: bool,
    pub(crate) supports_libretro_database: bool,
    pub(crate) supports_ram_writes: bool,
    pub(crate) supports_rom_patches: bool,
    pub(crate) formats: &'static [&'static str],
}

impl CheatCapabilities {
    pub(crate) fn for_system(system: ActiveSystem) -> Self {
        match system {
            ActiveSystem::GameBoy => Self {
                supports_user_cheats: true,
                supports_libretro_database: true,
                supports_ram_writes: true,
                supports_rom_patches: true,
                formats: &["GameShark", "Game Genie", "XPloder", "Raw"],
            },
            ActiveSystem::Nes => Self {
                supports_user_cheats: true,
                supports_libretro_database: true,
                supports_ram_writes: true,
                supports_rom_patches: true,
                formats: &["Game Genie", "GameShark", "Raw"],
            },
            ActiveSystem::Coleco => Self {
                supports_user_cheats: true,
                supports_libretro_database: false,
                supports_ram_writes: true,
                supports_rom_patches: false,
                formats: &["Raw"],
            },
            ActiveSystem::MasterSystem | ActiveSystem::GameGear => Self {
                supports_user_cheats: true,
                supports_libretro_database: true,
                supports_ram_writes: true,
                supports_rom_patches: true,
                formats: &["Raw", "Action Replay", "Game Genie"],
            },
            ActiveSystem::Sg1000 => Self {
                supports_user_cheats: true,
                supports_libretro_database: false,
                supports_ram_writes: true,
                supports_rom_patches: true,
                formats: &["Raw", "Action Replay", "Game Genie"],
            },
            ActiveSystem::GameBoyAdvance => Self {
                supports_user_cheats: true,
                supports_libretro_database: false,
                supports_ram_writes: true,
                supports_rom_patches: false,
                formats: &["Raw", "CodeBreaker/XPloder"],
            },
            ActiveSystem::WonderSwan => Self {
                supports_user_cheats: true,
                supports_libretro_database: false,
                supports_ram_writes: true,
                supports_rom_patches: false,
                formats: &["Raw"],
            },
            ActiveSystem::Pce => Self {
                supports_user_cheats: true,
                supports_libretro_database: true,
                supports_ram_writes: true,
                supports_rom_patches: false,
                formats: &["Raw logical", "Raw physical RAM"],
            },
        }
    }
}
