mod cheats;
mod data_models;
mod memory;
pub(crate) mod mods;
mod viewers;

pub(crate) use cheats::{BreakpointState, CheatState, LibretroAsyncResult};
pub(crate) use data_models::{
    ApuChannelDebug, ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugSection,
    GbGraphicsData, InputDebugInfo, NesGraphicsData, OamDebugInfo, PaletteDebugInfo,
    PaletteGroupDebug, PaletteRowDebug, RomDebugInfo, RomInfoSection, WatchHitDisplay,
    WatchpointDisplay,
};
pub(crate) use memory::{
    MemoryBookmark, MemoryByteDiff, MemorySearchMode, MemorySearchResult, MemoryViewerState,
    RomSearchResult, RomViewerState,
};
pub(crate) use mods::ModState;
pub(crate) use viewers::{PerfInfo, TileViewerState, TilemapViewerState};

use super::DisassemblyView;

#[derive(Clone, Copy)]
pub(crate) struct DebugDataRefs<'a> {
    pub(crate) cpu_debug: Option<&'a CpuDebugSnapshot>,
    pub(crate) perf_info: Option<&'a PerfInfo>,
    pub(crate) apu_debug: Option<&'a ApuDebugInfo>,
    pub(crate) oam_debug: Option<&'a OamDebugInfo>,
    pub(crate) palette_debug: Option<&'a PaletteDebugInfo>,
    pub(crate) rom_debug: Option<&'a RomDebugInfo>,
    pub(crate) input_debug: Option<&'a InputDebugInfo>,
    pub(crate) graphics_data: Option<&'a ConsoleGraphicsData>,
    pub(crate) disassembly_view: Option<&'a DisassemblyView>,
    pub(crate) memory_page: Option<&'a [(u16, u8)]>,
    pub(crate) rom_page: Option<&'a [(u32, u8)]>,
    pub(crate) rom_size: u32,
}

use crate::settings::{BindingAction, InputBindingAction, ShortcutAction};

pub(crate) struct DebugWindowState {
    pub(crate) memory: MemoryViewerState,
    pub(crate) bp: BreakpointState,
    pub(crate) rebinding_action: Option<InputBindingAction>,
    pub(crate) rebinding_shortcut: Option<ShortcutAction>,
    pub(crate) rebinding_gamepad: Option<BindingAction>,
    pub(crate) rebinding_gamepad_action: Option<crate::settings::GamepadAction>,
    pub(crate) rebinding_speedup: bool,
    pub(crate) rebinding_rewind: bool,
    pub(crate) last_disasm_pc: Option<u16>,
    pub(crate) tilemap: TilemapViewerState,
    pub(crate) tiles: TileViewerState,
    pub(crate) rom_viewer: RomViewerState,
    pub(crate) perf_history: crate::debug::perf_monitor::PerfHistory,
    pub(crate) settings_tab: usize,
    pub(crate) camera_devices: Vec<crate::camera::CameraDeviceInfo>,
    pub(crate) camera_device_error: Option<String>,
    pub(crate) camera_devices_needs_refresh: bool,
    pub(crate) cheat: CheatState,
    pub(crate) mod_state: ModState,
    pub(crate) layer_enable_bg: bool,
    pub(crate) layer_enable_window: bool,
    pub(crate) layer_enable_sprites: bool,
    pub(crate) tile_viewer_was_open: bool,
    pub(crate) tilemap_viewer_was_open: bool,
}

impl DebugWindowState {
    pub(crate) fn new() -> Self {
        Self {
            memory: MemoryViewerState::new(),
            bp: BreakpointState::new(),
            rebinding_action: None,
            rebinding_shortcut: None,
            rebinding_gamepad: None,
            rebinding_gamepad_action: None,
            rebinding_speedup: false,
            rebinding_rewind: false,
            last_disasm_pc: None,
            tilemap: TilemapViewerState::new(),
            tiles: TileViewerState::new(),
            rom_viewer: RomViewerState::new(),
            perf_history: crate::debug::perf_monitor::PerfHistory::new(),
            settings_tab: 0,
            camera_devices: Vec::new(),
            camera_device_error: None,
            camera_devices_needs_refresh: true,
            cheat: CheatState::new(),
            mod_state: ModState::new(),
            layer_enable_bg: true,
            layer_enable_window: true,
            layer_enable_sprites: true,
            tile_viewer_was_open: false,
            tilemap_viewer_was_open: false,
        }
    }
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    crc32fast::hash(bytes) as u64
}
