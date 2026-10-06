use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::test_support::{TestDirectory, test_directory};
use zeff_nes_core::hardware::cartridge::TimingMode;

#[derive(Clone, Copy)]
pub(super) enum Case {
    Nes(TimingMode),
    Sega(ActiveSystem, bool),
    Pce(bool),
    Ws(bool, Player),
}

pub(super) const CASES: [Case; 13] = [
    Case::Nes(TimingMode::Ntsc),
    Case::Nes(TimingMode::Pal),
    Case::Nes(TimingMode::Dendy),
    Case::Sega(ActiveSystem::MasterSystem, false),
    Case::Sega(ActiveSystem::MasterSystem, true),
    Case::Sega(ActiveSystem::Sg1000, false),
    Case::Sega(ActiveSystem::Sg1000, true),
    Case::Pce(false),
    Case::Pce(true),
    Case::Ws(false, Player::One),
    Case::Ws(false, Player::Two),
    Case::Ws(true, Player::One),
    Case::Ws(true, Player::Two),
];

impl Case {
    pub(super) fn label(self) -> String {
        match self {
            Self::Nes(timing) => format!("nes-{timing:?}"),
            Self::Sega(system, pal) => format!("{system:?}-{}", if pal { "pal" } else { "ntsc" }),
            Self::Pce(sgx) => if sgx { "supergrafx" } else { "pce" }.into(),
            Self::Ws(color, player) => {
                format!("{}-{player:?}", if color { "wsc" } else { "ws" })
            }
        }
    }

    pub(super) fn player(self) -> Player {
        match self {
            Self::Ws(_, player) => player,
            _ => Player::One,
        }
    }

    pub(super) fn load(self) -> (TestDirectory, EmuBackend) {
        match self {
            Self::Sega(system, pal) => sega8_tests::loaded(system, pal),
            Self::Pce(sgx) => pce_tests::loaded(sgx),
            Self::Ws(color, _) => ws_tests::loaded(color),
            Self::Nes(timing) => {
                let directory = test_directory("rollback-cost-nes").unwrap();
                let path = directory.path().join("game.nes");
                std::fs::write(&path, crate::netplay::proof::fixture_rom(timing)).unwrap();
                let backend = load_backend_from_rom_source(
                    ActiveSystem::Nes,
                    &path,
                    &path,
                    None,
                    BackendLoadConfig {
                        sample_rate: Some(48_000),
                        nes_load_battery_sram: false,
                        ..Default::default()
                    },
                )
                .unwrap()
                .backend;
                (directory, backend)
            }
        }
    }
}
