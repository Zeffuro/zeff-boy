mod apu_viewer;
mod breakpoints;
mod breakpoints_window;
mod cheats_window;
mod disasm_window;
mod disassembler;
pub(crate) mod dock;
pub(crate) mod export;
mod fps;
mod input_viewer;
mod libretro_cheats;
pub(crate) mod memory_viewer;
mod menu_bar;
mod oam_viewer;
mod palette_viewer;
pub(crate) mod perf_monitor;
mod rom_info;
mod rom_viewer;
mod settings_window;
mod tile_viewer;
mod tilemap_viewer;
pub(crate) mod toast;
pub(crate) mod types;
mod ui;

pub(crate) use breakpoints::DebugController;
pub(crate) use disassembler::{DisassemblyView, disassemble_around};
pub(crate) use dock::{
    DebugTab, DebugTabViewer, TabDataRequirements, compute_tab_requirements,
    create_default_dock_state, create_dock_from_saved_tabs,
    create_ide_dock_state, ensure_game_view_tab, has_game_view_tab, save_open_tabs,
    sync_show_flags,
};
pub(crate) use fps::FpsTracker;
pub(crate) use toast::ToastManager;
pub(crate) use types::{
    BreakpointState, CheatState, DebugInfo, DebugViewerData, DebugWindowState, MemorySearchResult,
    OpcodeLog, PpuSnapshot, RomInfoViewData, RomSearchResult, TileViewerState, TilemapViewerState,
    WatchpointInfo,
};
pub(crate) use ui::{DebugUiActions, MenuActions, draw_menu_bar, draw_settings_window};
