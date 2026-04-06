use crate::cheats::CheatCode;
use crate::debug::common::WatchType;
use crate::libretro_common::LibretroPlatform;
use crate::libretro_metadata::MetadataRefreshStats;

pub(crate) enum LibretroAsyncResult {
    FileList(anyhow::Result<Vec<String>>),
    Downloaded {
        filename: String,
        result: anyhow::Result<String>,
    },
    MetadataRefreshed(anyhow::Result<MetadataRefreshStats>),
}

pub(crate) struct CheatState {
    pub(crate) user_codes: Vec<CheatCode>,
    pub(crate) libretro_codes: Vec<CheatCode>,
    pub(crate) input: String,
    pub(crate) name_input: String,
    pub(crate) parse_error: Option<String>,
    pub(crate) rom_title: Option<String>,
    pub(crate) rom_crc32: Option<u32>,
    pub(crate) rom_metadata_title: Option<String>,
    pub(crate) rom_metadata_rom_name: Option<String>,
    pub(crate) libretro_platform: LibretroPlatform,
    pub(crate) active_system: crate::emu_backend::ActiveSystem,
    pub(crate) libretro_search_hints: Vec<String>,
    pub(crate) libretro_search: String,
    pub(crate) libretro_results: Vec<String>,
    pub(crate) libretro_status: Option<String>,
    pub(crate) libretro_file_list: Option<Vec<String>>,
    pub(crate) libretro_show: bool,
    pub(crate) cheats_dirty: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) libretro_rx: Option<crossbeam_channel::Receiver<LibretroAsyncResult>>,
    pub(crate) libretro_busy: bool,
}

impl CheatState {
    pub(crate) fn new() -> Self {
        Self {
            user_codes: Vec::new(),
            libretro_codes: Vec::new(),
            input: String::new(),
            name_input: String::new(),
            parse_error: None,
            rom_title: None,
            rom_crc32: None,
            rom_metadata_title: None,
            rom_metadata_rom_name: None,
            libretro_platform: LibretroPlatform::Gb,
            active_system: crate::emu_backend::ActiveSystem::GameBoy,
            libretro_search_hints: Vec::new(),
            libretro_search: String::new(),
            libretro_results: Vec::new(),
            libretro_status: None,
            libretro_file_list: None,
            libretro_show: false,
            cheats_dirty: true,
            #[cfg(not(target_arch = "wasm32"))]
            libretro_rx: None,
            libretro_busy: false,
        }
    }
}

pub(crate) struct BreakpointState {
    pub(crate) input: String,
    pub(crate) watchpoint_input: String,
    pub(crate) watchpoint_type: WatchType,
}

impl BreakpointState {
    pub(crate) fn new() -> Self {
        Self {
            input: String::new(),
            watchpoint_input: String::new(),
            watchpoint_type: WatchType::Write,
        }
    }
}
