mod apu_viewer;
mod breakpoints_window;
mod cheats_window;
pub(crate) mod common;
mod disasm_window;
mod disassembler;
pub(crate) mod dock;
pub(crate) mod export;
mod fps;
pub(crate) mod hex_viewer;
mod input_viewer;
mod libretro_cheats;
mod memory_viewer;
mod menu_bar;
mod nes_tile_viewer;
mod nes_tilemap_viewer;
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
pub(crate) mod ui_helpers;
mod ui;

pub(crate) use types::{
    ApuChannelDebug, ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugSection,
    GbGraphicsData, InputDebugInfo, NesGraphicsData, OamDebugInfo, PaletteDebugInfo,
    PaletteGroupDebug, PaletteRowDebug, RomDebugInfo, RomInfoSection, WatchHitDisplay,
    WatchpointDisplay,
};
pub(crate) use disassembler::{DisassemblyView, disassemble_around, nes_disassemble_around};
pub(crate) use dock::{
    DebugTab, DebugTabViewer, compute_tab_requirements,
    create_default_dock_state, create_dock_from_saved_tabs,
    create_ide_dock_state, ensure_game_view_tab, is_tab_open, save_open_tabs,
};
pub(crate) use fps::FpsTracker;
pub(crate) use toast::ToastManager;
pub(crate) use types::{
    BreakpointState, CheatState, DebugWindowState, MemorySearchResult,
    PerfInfo, RomSearchResult, TileViewerState, TilemapViewerState,
};
pub(crate) use ui::{DebugUiActions, MenuAction, MenuBarContext, MenuBarResult, draw_menu_bar, draw_settings_window};
