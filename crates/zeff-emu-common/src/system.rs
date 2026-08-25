use std::fmt;
use std::path::Path;

const GB_ROM_EXTENSIONS: [&str; 3] = ["gb", "gbc", "sgb"];
const GBA_ROM_EXTENSIONS: [&str; 1] = ["gba"];
const NES_ROM_EXTENSIONS: [&str; 2] = ["nes", "fds"];
#[cfg(not(target_arch = "wasm32"))]
const PCE_ROM_EXTENSIONS: &[&str] = &["pce", "cue", "chd", "iso"];
#[cfg(target_arch = "wasm32")]
const PCE_ROM_EXTENSIONS: &[&str] = &["pce"];
const WS_ROM_EXTENSIONS: [&str; 2] = ["ws", "wsc"];
const SMS_ROM_EXTENSIONS: [&str; 1] = ["sms"];
const GG_ROM_EXTENSIONS: [&str; 1] = ["gg"];
const SG_ROM_EXTENSIONS: [&str; 2] = ["sg", "sc"];
#[cfg(not(target_arch = "wasm32"))]
const ARCHIVE_EXTENSION_LIST: [&str; 3] = ["zip", "7z", "rar"];
#[cfg(target_arch = "wasm32")]
const ARCHIVE_EXTENSION_LIST: [&str; 1] = ["zip"];
#[cfg(not(target_arch = "wasm32"))]
const SUPPORTED_EXTENSIONS_LABEL: &str = ".gb, .gbc, .sgb, .gba, .nes, .fds, .pce, .cue, .chd, .iso, .ws, .wsc, .sms, .gg, .sg, .sc, .zip, .7z, .rar";
#[cfg(target_arch = "wasm32")]
const SUPPORTED_EXTENSIONS_LABEL: &str =
    ".gb, .gbc, .sgb, .gba, .nes, .fds, .pce, .ws, .wsc, .sms, .gg, .sg, .sc, .zip";
const SUPPORTED_SYSTEM_VALUES: &str = "gb|gba|nes|pce|ws|sms|gg|sg";

pub const RGBA_BYTES_PER_PIXEL: usize = 4;
pub const GAME_BOY_SCREEN_SIZE: (u32, u32) = (160, 144);
pub const SUPER_GAME_BOY_SCREEN_SIZE: (u32, u32) = (256, 224);
pub const GBA_SCREEN_SIZE: (u32, u32) = (240, 160);
pub const NES_SCREEN_SIZE: (u32, u32) = (256, 240);
pub const PCE_SCREEN_SIZE: (u32, u32) = (640, 480);
pub const WS_SCREEN_SIZE: (u32, u32) = (224, 144);
pub const SMS_SCREEN_SIZE: (u32, u32) = (256, 192);
pub const GG_SCREEN_SIZE: (u32, u32) = (160, 144);
pub const SG_SCREEN_SIZE: (u32, u32) = SMS_SCREEN_SIZE;

pub const NANOS_PER_SECOND: f64 = 1_000_000_000.0;
pub const GAME_BOY_FRAME_DURATION_NS: u64 = 16_742_706;
pub const GBA_FRAME_DURATION_NS: u64 = GAME_BOY_FRAME_DURATION_NS;
pub const NES_FRAME_DURATION_NS: u64 = 16_639_267;
pub const PROVISIONAL_PCE_263_LINE_FRAME_DURATION_NS: u64 = 16_715_111;
pub const WS_FRAME_DURATION_NS: u64 = 13_250_298;
pub const SMS_FRAME_DURATION_NS: u64 = 16_666_667;
pub const GG_FRAME_DURATION_NS: u64 = SMS_FRAME_DURATION_NS;
pub const SG_FRAME_DURATION_NS: u64 = SMS_FRAME_DURATION_NS;

pub const fn rgba_framebuffer_len(screen_size: (u32, u32)) -> usize {
    screen_size.0 as usize * screen_size.1 as usize * RGBA_BYTES_PER_PIXEL
}

pub const ROM_EXTENSIONS: &[&str] = &[
    GB_ROM_EXTENSIONS[0],
    GB_ROM_EXTENSIONS[1],
    GB_ROM_EXTENSIONS[2],
    GBA_ROM_EXTENSIONS[0],
    NES_ROM_EXTENSIONS[0],
    NES_ROM_EXTENSIONS[1],
    PCE_ROM_EXTENSIONS[0],
    #[cfg(not(target_arch = "wasm32"))]
    PCE_ROM_EXTENSIONS[1],
    #[cfg(not(target_arch = "wasm32"))]
    PCE_ROM_EXTENSIONS[2],
    #[cfg(not(target_arch = "wasm32"))]
    PCE_ROM_EXTENSIONS[3],
    WS_ROM_EXTENSIONS[0],
    WS_ROM_EXTENSIONS[1],
    SMS_ROM_EXTENSIONS[0],
    GG_ROM_EXTENSIONS[0],
    SG_ROM_EXTENSIONS[0],
    SG_ROM_EXTENSIONS[1],
];
pub const ROM_AND_ARCHIVE_EXTENSIONS: &[&str] = &[
    GB_ROM_EXTENSIONS[0],
    GB_ROM_EXTENSIONS[1],
    GB_ROM_EXTENSIONS[2],
    GBA_ROM_EXTENSIONS[0],
    NES_ROM_EXTENSIONS[0],
    NES_ROM_EXTENSIONS[1],
    PCE_ROM_EXTENSIONS[0],
    #[cfg(not(target_arch = "wasm32"))]
    PCE_ROM_EXTENSIONS[1],
    #[cfg(not(target_arch = "wasm32"))]
    PCE_ROM_EXTENSIONS[2],
    #[cfg(not(target_arch = "wasm32"))]
    PCE_ROM_EXTENSIONS[3],
    WS_ROM_EXTENSIONS[0],
    WS_ROM_EXTENSIONS[1],
    SMS_ROM_EXTENSIONS[0],
    GG_ROM_EXTENSIONS[0],
    SG_ROM_EXTENSIONS[0],
    SG_ROM_EXTENSIONS[1],
    ARCHIVE_EXTENSION_LIST[0],
    #[cfg(not(target_arch = "wasm32"))]
    ARCHIVE_EXTENSION_LIST[1],
    #[cfg(not(target_arch = "wasm32"))]
    ARCHIVE_EXTENSION_LIST[2],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
pub enum System {
    Gb,
    Gba,
    Nes,
    Pce,
    Ws,
    Sms,
    Gg,
    Sg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
pub enum CoreFamily {
    GameBoy,
    GameBoyAdvance,
    Nes,
    PcEngine,
    WonderSwan,
    Sega8,
}

#[allow(non_upper_case_globals)]
impl System {
    pub const GameBoy: Self = Self::Gb;
    pub const GameBoyAdvance: Self = Self::Gba;
    pub const WonderSwan: Self = Self::Ws;
    pub const PcEngine: Self = Self::Pce;
    pub const MasterSystem: Self = Self::Sms;
    pub const GameGear: Self = Self::Gg;
    pub const Sg1000: Self = Self::Sg;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemSpec {
    pub system: System,
    pub core_family: CoreFamily,
    pub file_dialog_filter_name: &'static str,
    pub code: &'static str,
    pub short_code: &'static str,
    pub storage_subdir: &'static str,
    pub state_extension: &'static str,
    pub screen_size: (u32, u32),
    pub frame_duration_ns: u64,
    pub rom_extensions: &'static [&'static str],
}

const SYSTEM_SPECS: &[SystemSpec] = &[
    SystemSpec {
        system: System::Gb,
        core_family: CoreFamily::GameBoy,
        file_dialog_filter_name: "Game Boy ROMs",
        code: "gb",
        short_code: "gb",
        storage_subdir: "gbc",
        state_extension: "gbstate",
        screen_size: GAME_BOY_SCREEN_SIZE,
        frame_duration_ns: GAME_BOY_FRAME_DURATION_NS,
        rom_extensions: &GB_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: System::Gba,
        core_family: CoreFamily::GameBoyAdvance,
        file_dialog_filter_name: "Game Boy Advance ROMs",
        code: "gba",
        short_code: "gba",
        storage_subdir: "gba",
        state_extension: "gbastate",
        screen_size: GBA_SCREEN_SIZE,
        frame_duration_ns: GBA_FRAME_DURATION_NS,
        rom_extensions: &GBA_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: System::Nes,
        core_family: CoreFamily::Nes,
        file_dialog_filter_name: "NES ROMs",
        code: "nes",
        short_code: "nes",
        storage_subdir: "nes",
        state_extension: "nstate",
        screen_size: NES_SCREEN_SIZE,
        frame_duration_ns: NES_FRAME_DURATION_NS,
        rom_extensions: &NES_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: System::Pce,
        core_family: CoreFamily::PcEngine,
        file_dialog_filter_name: "PC Engine ROMs",
        code: "pce",
        short_code: "pce",
        storage_subdir: "pce",
        state_extension: "pcestate",
        screen_size: PCE_SCREEN_SIZE,
        frame_duration_ns: PROVISIONAL_PCE_263_LINE_FRAME_DURATION_NS,
        rom_extensions: PCE_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: System::Ws,
        core_family: CoreFamily::WonderSwan,
        file_dialog_filter_name: "WonderSwan ROMs",
        code: "ws",
        short_code: "ws",
        storage_subdir: "ws",
        state_extension: "wsstate",
        screen_size: WS_SCREEN_SIZE,
        frame_duration_ns: WS_FRAME_DURATION_NS,
        rom_extensions: &WS_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: System::Sms,
        core_family: CoreFamily::Sega8,
        file_dialog_filter_name: "Sega Master System ROMs",
        code: "sms",
        short_code: "sms",
        storage_subdir: "sms",
        state_extension: "smsstate",
        screen_size: SMS_SCREEN_SIZE,
        frame_duration_ns: SMS_FRAME_DURATION_NS,
        rom_extensions: &SMS_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: System::Gg,
        core_family: CoreFamily::Sega8,
        file_dialog_filter_name: "Game Gear ROMs",
        code: "gg",
        short_code: "gg",
        storage_subdir: "gg",
        state_extension: "ggstate",
        screen_size: GG_SCREEN_SIZE,
        frame_duration_ns: GG_FRAME_DURATION_NS,
        rom_extensions: &GG_ROM_EXTENSIONS,
    },
    SystemSpec {
        system: System::Sg,
        core_family: CoreFamily::Sega8,
        file_dialog_filter_name: "SG-1000 ROMs",
        code: "sg",
        short_code: "sg",
        storage_subdir: "sg",
        state_extension: "sgstate",
        screen_size: SG_SCREEN_SIZE,
        frame_duration_ns: SG_FRAME_DURATION_NS,
        rom_extensions: &SG_ROM_EXTENSIONS,
    },
];

impl SystemSpec {
    pub const fn framebuffer_len(self) -> usize {
        rgba_framebuffer_len(self.screen_size)
    }

    pub fn target_fps(self) -> f64 {
        NANOS_PER_SECOND / self.frame_duration_ns as f64
    }
}

impl System {
    pub fn specs() -> &'static [SystemSpec] {
        SYSTEM_SPECS
    }

    pub fn spec(self) -> &'static SystemSpec {
        Self::specs()
            .iter()
            .find(|spec| spec.system == self)
            .expect("System must have a SystemSpec")
    }

    pub fn code(self) -> &'static str {
        self.spec().code
    }

    pub fn short_code(self) -> &'static str {
        self.spec().short_code
    }

    pub fn storage_subdir(self) -> &'static str {
        self.spec().storage_subdir
    }

    pub fn state_extension(self) -> &'static str {
        self.spec().state_extension
    }

    pub fn core_family(self) -> CoreFamily {
        self.spec().core_family
    }

    pub fn screen_size(self) -> (u32, u32) {
        self.spec().screen_size
    }

    pub fn framebuffer_len(self) -> usize {
        self.spec().framebuffer_len()
    }

    pub fn frame_duration_ns(self) -> u64 {
        self.spec().frame_duration_ns
    }

    pub fn target_fps(self) -> f64 {
        self.spec().target_fps()
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::from_extension(&ext)
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        Self::specs()
            .iter()
            .find(|spec| spec.rom_extensions.contains(&ext.as_str()))
            .map(|spec| spec.system)
    }

    pub fn from_code(value: &str) -> Option<Self> {
        let value = value.trim();
        Self::specs()
            .iter()
            .find(|spec| spec.code == value)
            .map(|spec| spec.system)
    }

    pub fn supported_values() -> &'static str {
        SUPPORTED_SYSTEM_VALUES
    }

    pub fn supported_extensions() -> &'static str {
        SUPPORTED_EXTENSIONS_LABEL
    }
}

impl std::str::FromStr for System {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_code(value).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid system '{value}', expected {}",
                Self::supported_values()
            )
        })
    }
}

impl fmt::Display for System {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

pub fn system_specs() -> &'static [SystemSpec] {
    SYSTEM_SPECS
}

pub fn archive_extensions() -> &'static [&'static str] {
    &ARCHIVE_EXTENSION_LIST
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn detects_supported_rom_extensions() {
        assert_eq!(
            System::from_path(&PathBuf::from("GAME.GB")),
            Some(System::Gb)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.gbc")),
            Some(System::Gb)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.sgb")),
            Some(System::Gb)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.gba")),
            Some(System::Gba)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.nes")),
            Some(System::Nes)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.fds")),
            Some(System::Nes)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.pce")),
            Some(System::Pce)
        );
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(
            System::from_path(&PathBuf::from("game.cue")),
            Some(System::Pce)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.chd")),
            Some(System::Pce)
        );
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(
            System::from_path(&PathBuf::from("game.iso")),
            Some(System::Pce)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.ws")),
            Some(System::Ws)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.wsc")),
            Some(System::Ws)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.sms")),
            Some(System::Sms)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.gg")),
            Some(System::Gg)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.sg")),
            Some(System::Sg)
        );
        assert_eq!(
            System::from_path(&PathBuf::from("game.sc")),
            Some(System::Sg)
        );
    }

    #[test]
    fn package_archive_is_native_only_and_never_a_rom_extension() {
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert_eq!(archive_extensions(), &["zip", "7z", "rar"]);
            assert!(ROM_AND_ARCHIVE_EXTENSIONS.contains(&"7z"));
            assert!(ROM_AND_ARCHIVE_EXTENSIONS.contains(&"rar"));
        }
        #[cfg(target_arch = "wasm32")]
        {
            assert_eq!(archive_extensions(), &["zip"]);
            assert!(!ROM_AND_ARCHIVE_EXTENSIONS.contains(&"7z"));
        }
        assert!(!ROM_EXTENSIONS.contains(&"7z"));
        assert!(!ROM_EXTENSIONS.contains(&"rar"));
        assert_eq!(System::from_path(Path::new("disc.7z")), None);
    }

    #[test]
    fn specs_cover_supported_rom_extensions() {
        let from_specs = system_specs()
            .iter()
            .flat_map(|spec| spec.rom_extensions.iter().copied())
            .collect::<BTreeSet<_>>();
        let from_constant = ROM_EXTENSIONS.iter().copied().collect::<BTreeSet<_>>();

        assert_eq!(from_specs, from_constant);
        for spec in system_specs() {
            assert_eq!(System::from_extension(spec.short_code), Some(spec.system));
            assert_eq!(System::from_code(spec.code), Some(spec.system));
            assert!(!spec.storage_subdir.is_empty());
            assert!(!spec.state_extension.is_empty());
            assert!(!spec.file_dialog_filter_name.is_empty());
            assert!(spec.frame_duration_ns > 0);
            assert!(spec.target_fps() > 0.0);
            assert_eq!(
                spec.framebuffer_len(),
                rgba_framebuffer_len(spec.screen_size)
            );
        }
    }

    #[test]
    fn app_aliases_match_romtest_style_variants() {
        assert_eq!(System::GameBoy, System::Gb);
        assert_eq!(System::GameBoyAdvance, System::Gba);
        assert_eq!(System::WonderSwan, System::Ws);
        assert_eq!(System::PcEngine, System::Pce);
        assert_eq!(System::MasterSystem, System::Sms);
        assert_eq!(System::GameGear, System::Gg);
        assert_eq!(System::Sg1000, System::Sg);
    }

    #[test]
    fn systems_map_to_expected_core_families() {
        assert_eq!(System::Gb.core_family(), CoreFamily::GameBoy);
        assert_eq!(System::Gba.core_family(), CoreFamily::GameBoyAdvance);
        assert_eq!(System::Nes.core_family(), CoreFamily::Nes);
        assert_eq!(System::Pce.core_family(), CoreFamily::PcEngine);
        assert_eq!(System::Ws.core_family(), CoreFamily::WonderSwan);
        assert_eq!(System::Sms.core_family(), CoreFamily::Sega8);
        assert_eq!(System::Gg.core_family(), CoreFamily::Sega8);
        assert_eq!(System::Sg.core_family(), CoreFamily::Sega8);
    }

    #[test]
    fn pce_uses_the_provisional_263_line_nominal_frame_duration() {
        assert_eq!(
            System::Pce.frame_duration_ns(),
            PROVISIONAL_PCE_263_LINE_FRAME_DURATION_NS
        );
        assert_eq!(PROVISIONAL_PCE_263_LINE_FRAME_DURATION_NS, 16_715_111);
    }
}
