use std::path::PathBuf;

use crate::emu_backend::ActiveSystem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArchiveRomEntry {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) system: ActiveSystem,
    pub(crate) uncompressed_size: u64,
}

impl ArchiveRomEntry {
    pub(crate) fn display_label(&self) -> String {
        format!(
            "{}  ({}, {})",
            self.name,
            system_label(self.system),
            format_size(self.uncompressed_size)
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PendingArchiveSelection {
    pub(crate) archive_path: PathBuf,
    pub(crate) entries: Vec<ArchiveRomEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ArchiveSelectionAction {
    Load {
        archive_path: PathBuf,
        entry_index: usize,
    },
    Cancel,
}

fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;

    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn system_label(system: ActiveSystem) -> &'static str {
    match system {
        ActiveSystem::GameBoy => "Game Boy",
        ActiveSystem::GameBoyAdvance => "Game Boy Advance",
        ActiveSystem::Nes => "NES",
        ActiveSystem::Coleco => "ColecoVision",
        ActiveSystem::Pce => "PC Engine",
        ActiveSystem::WonderSwan => "WonderSwan",
        ActiveSystem::MasterSystem => "Master System",
        ActiveSystem::GameGear => "Game Gear",
        ActiveSystem::Sg1000 => "SG-1000/SC-3000",
    }
}
