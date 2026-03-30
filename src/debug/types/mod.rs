mod cheats;
mod data_models;
mod memory;
mod viewers;

pub(crate) use cheats::{BreakpointState, CheatState};
pub(crate) use data_models::{
    ApuChannelDebug, ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugSection,
    GbGraphicsData, InputDebugInfo, NesGraphicsData, OamDebugInfo, PaletteDebugInfo,
    PaletteGroupDebug, PaletteRowDebug, RomDebugInfo, RomInfoSection, WatchHitDisplay,
    WatchpointDisplay,
};
pub(crate) use memory::{
    MemorySearchMode, MemorySearchResult, MemoryViewerState, RomSearchResult, RomViewerState,
};
pub(crate) use viewers::{PerfInfo, TileViewerState, TilemapViewerState};

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
    pub(crate) layer_enable_bg: bool,
    pub(crate) layer_enable_window: bool,
    pub(crate) layer_enable_sprites: bool,
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
            layer_enable_bg: true,
            layer_enable_window: true,
            layer_enable_sprites: true,
        }
    }
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    crc32fast::hash(bytes) as u64
}

