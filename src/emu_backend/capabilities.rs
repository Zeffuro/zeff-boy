use zeff_emu_common::memory::MemoryRegionDescriptor;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::system::CoreFamily;

use crate::input::HostButton;

use super::ActiveSystem;

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
