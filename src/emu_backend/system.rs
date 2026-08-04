use std::path::Path;

const GB_ROM_EXTENSIONS: [&str; 3] = ["gb", "gbc", "sgb"];
const GBA_ROM_EXTENSIONS: [&str; 1] = ["gba"];
const NES_ROM_EXTENSIONS: [&str; 1] = ["nes"];
const WS_ROM_EXTENSIONS: [&str; 2] = ["ws", "wsc"];
const ARCHIVE_EXTENSION_LIST: [&str; 1] = ["zip"];
const SUPPORTED_EXTENSIONS_LABEL: &str = ".gb, .gbc, .sgb, .gba, .nes, .ws, .wsc, .zip";

pub(crate) const ROM_EXTENSIONS: &[&str] = &[
    GB_ROM_EXTENSIONS[0],
    GB_ROM_EXTENSIONS[1],
    GB_ROM_EXTENSIONS[2],
    GBA_ROM_EXTENSIONS[0],
    NES_ROM_EXTENSIONS[0],
    WS_ROM_EXTENSIONS[0],
    WS_ROM_EXTENSIONS[1],
];
pub(crate) const ROM_AND_ARCHIVE_EXTENSIONS: &[&str] = &[
    GB_ROM_EXTENSIONS[0],
    GB_ROM_EXTENSIONS[1],
    GB_ROM_EXTENSIONS[2],
    GBA_ROM_EXTENSIONS[0],
    NES_ROM_EXTENSIONS[0],
    WS_ROM_EXTENSIONS[0],
    WS_ROM_EXTENSIONS[1],
    ARCHIVE_EXTENSION_LIST[0],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActiveSystem {
    GameBoy,
    GameBoyAdvance,
    Nes,
    WonderSwan,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SystemSpec {
    pub(crate) system: ActiveSystem,
    pub(crate) file_dialog_filter_name: &'static str,
    pub(crate) short_code: &'static str,
    pub(crate) storage_subdir: &'static str,
    pub(crate) state_extension: &'static str,
    pub(crate) screen_size: (u32, u32),
    pub(crate) rom_extensions: &'static [&'static str],
}

const SYSTEM_SPECS: &[SystemSpec] = &[
    SystemSpec {
        system: ActiveSystem::GameBoy,
        file_dialog_filter_name: "Game Boy ROMs",
        short_code: "gb",
        storage_subdir: "gbc",
        state_extension: "gbstate",
        screen_size: (160, 144),
        rom_extensions: &GB_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: ActiveSystem::GameBoyAdvance,
        file_dialog_filter_name: "Game Boy Advance ROMs",
        short_code: "gba",
        storage_subdir: "gba",
        state_extension: "gbastate",
        screen_size: (240, 160),
        rom_extensions: &GBA_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: ActiveSystem::Nes,
        file_dialog_filter_name: "NES ROMs",
        short_code: "nes",
        storage_subdir: "nes",
        state_extension: "nstate",
        screen_size: (256, 240),
        rom_extensions: &NES_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: ActiveSystem::WonderSwan,
        file_dialog_filter_name: "WonderSwan ROMs",
        short_code: "ws",
        storage_subdir: "ws",
        state_extension: "wsstate",
        screen_size: (224, 144),
        rom_extensions: &WS_ROM_EXTENSIONS,
    },
];

pub(crate) fn system_specs() -> &'static [SystemSpec] {
    SYSTEM_SPECS
}

impl ActiveSystem {
    pub(crate) fn spec(self) -> &'static SystemSpec {
        SYSTEM_SPECS
            .iter()
            .find(|spec| spec.system == self)
            .expect("ActiveSystem must have a SystemSpec")
    }

    pub(crate) fn storage_subdir(self) -> &'static str {
        self.spec().storage_subdir
    }

    pub(crate) fn short_code(self) -> &'static str {
        self.spec().short_code
    }

    pub(crate) fn state_extension(self) -> &'static str {
        self.spec().state_extension
    }

    pub(crate) fn screen_size(self) -> (u32, u32) {
        self.spec().screen_size
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::from_extension(&ext)
    }

    pub(crate) fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        SYSTEM_SPECS
            .iter()
            .find(|spec| spec.rom_extensions.contains(&ext.as_str()))
            .map(|spec| spec.system)
    }

    pub(crate) fn supported_extensions() -> &'static str {
        SUPPORTED_EXTENSIONS_LABEL
    }
}

pub(crate) fn archive_extensions() -> &'static [&'static str] {
    &ARCHIVE_EXTENSION_LIST
}
