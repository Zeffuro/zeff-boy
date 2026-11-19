#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InputSystem {
    GameBoy,
    GameBoyAdvance,
    Md,
    Nes,
    MasterSystem,
    GameGear,
    Sg1000,
    Pce,
    WonderSwan,
    Coleco,
}

impl InputSystem {
    pub(crate) const ALL: [Self; 10] = [
        Self::GameBoy,
        Self::GameBoyAdvance,
        Self::Md,
        Self::Nes,
        Self::MasterSystem,
        Self::GameGear,
        Self::Sg1000,
        Self::Pce,
        Self::WonderSwan,
        Self::Coleco,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::GameBoy => "Game Boy / Game Boy Color",
            Self::GameBoyAdvance => "Game Boy Advance",
            Self::Md => "Mega Drive / Genesis",
            Self::Nes => "NES / Famicom Disk System",
            Self::MasterSystem => "Master System",
            Self::GameGear => "Game Gear",
            Self::Sg1000 => "SG-1000",
            Self::Pce => "PC Engine",
            Self::WonderSwan => "WonderSwan",
            Self::Coleco => "ColecoVision",
        }
    }

    pub(super) fn code(self) -> &'static str {
        match self {
            Self::GameBoy => "gb",
            Self::GameBoyAdvance => "gba",
            Self::Md => "md",
            Self::Nes => "nes",
            Self::MasterSystem => "sms",
            Self::GameGear => "gg",
            Self::Sg1000 => "sg1000",
            Self::Pce => "pce",
            Self::WonderSwan => "ws",
            Self::Coleco => "coleco",
        }
    }
}

impl From<crate::emu_backend::ActiveSystem> for InputSystem {
    fn from(system: crate::emu_backend::ActiveSystem) -> Self {
        use crate::emu_backend::ActiveSystem;
        match system {
            ActiveSystem::GameBoy => Self::GameBoy,
            ActiveSystem::GameBoyAdvance => Self::GameBoyAdvance,
            ActiveSystem::Md => Self::Md,
            ActiveSystem::Nes => Self::Nes,
            ActiveSystem::MasterSystem => Self::MasterSystem,
            ActiveSystem::GameGear => Self::GameGear,
            ActiveSystem::Sg1000 => Self::Sg1000,
            ActiveSystem::Pce => Self::Pce,
            ActiveSystem::WonderSwan => Self::WonderSwan,
            ActiveSystem::Coleco => Self::Coleco,
        }
    }
}
