mod apu_viewer;
mod breakpoints_window;
mod cheats_window;
pub(crate) mod common;
mod console;
mod data_inspector;
mod disasm_window;
mod disassembler;
pub(crate) mod dock;
pub(crate) mod export;
mod fps;
mod gba_tile_viewer;
mod gba_tilemap_viewer;
pub(crate) mod hex_search;
pub(crate) mod hex_viewer;
mod input_viewer;
mod libretro_cheats;
mod memory_viewer;
mod menu_bar;
mod mods_window;
mod nes_tile_viewer;
mod nes_tilemap_viewer;
mod oam_viewer;
mod palette_viewer;
pub(crate) mod perf_monitor;
mod rom_info;
mod rom_viewer;
mod settings_window;
mod symbol_browser;
mod tile_viewer;
mod tilemap_viewer;
pub(crate) mod toast;
pub(crate) mod types;
mod ui;
pub(crate) mod ui_helpers;

pub(crate) use console::{ConsoleReadSpace, DebugConsoleState};
pub(crate) use disassembler::{
    DisassembledLine, DisassemblyTarget, DisassemblyView, disassemble_around,
    gba_disassemble_around, nes_disassemble_around, z80_disassemble_around,
};
pub(crate) use dock::{
    DebugTab, DebugTabViewer, activate_dock_tab, compute_tab_requirements,
    create_debugger_dock_state, create_default_dock_state, create_ide_dock_state,
    ensure_game_view_tab, is_tab_open, restore_dock_layout, save_open_tabs, serialize_dock_layout,
};
pub(crate) use fps::FpsTracker;
pub(crate) use toast::ToastManager;
pub(crate) use types::{
    ApuChannelDebug, ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugSection,
    GbGraphicsData, GbaGraphicsData, InputDebugInfo, NesGraphicsData, OamDebugInfo,
    PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug, RecentOpcodeDisplay, RomDebugInfo,
    RomInfoSection, Sega8GraphicsData, WatchHitDisplay, WatchpointDisplay,
};
pub(crate) use types::{
    BreakpointState, CheatState, DebugDataRefs, DebugWindowState, LibretroAsyncResult,
    MemorySearchMode, MemorySearchResult, MemoryViewerState, PerfInfo, RomSearchResult,
    RomViewerState, TileViewerState, TilemapViewerState,
};
#[cfg(target_arch = "wasm32")]
pub(crate) use ui::draw_settings_window;
pub(crate) use ui::{
    DebugUiActions, MenuAction, MenuBarContext, MenuBarResult, SettingsContext, draw_menu_bar,
    draw_settings_content,
};
