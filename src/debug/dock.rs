use egui_dock::{DockState, TabViewer};

use super::apu_viewer::draw_apu_viewer_content;
use super::breakpoints_window::draw_breakpoints_content;
use super::cheats_window::draw_cheats_content;
use super::disasm_window::draw_disassembler_content;
use super::memory_viewer::draw_memory_viewer_content;
use super::oam_viewer::draw_oam_viewer_content;
use super::palette_viewer::draw_palette_viewer_content;
use super::perf_monitor::draw_performance_content;
use super::rom_info::draw_rom_info_content;
use super::tile_viewer::draw_tile_viewer_content;
use super::tilemap_viewer::draw_tilemap_viewer_content;
use super::ui::draw_debug_ui_content;
use super::{DebugInfo, DebugUiActions, DebugViewerData, DebugWindowState, DisassemblyView, RomInfoViewData};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DebugTab {
    CpuDebug,
    ApuViewer,
    RomInfo,
    Disassembler,
    MemoryViewer,
    TileViewer,
    TilemapViewer,
    OamViewer,
    PaletteViewer,
    Performance,
    Breakpoints,
    Cheats,
}

impl DebugTab {
    fn title(self) -> &'static str {
        match self {
            Self::CpuDebug => "CPU / Debug",
            Self::ApuViewer => "APU / Sound",
            Self::RomInfo => "ROM Info",
            Self::Disassembler => "Disassembler",
            Self::MemoryViewer => "Memory Viewer",
            Self::TileViewer => "Tile Data",
            Self::TilemapViewer => "Tile Map",
            Self::OamViewer => "OAM / Sprites",
            Self::PaletteViewer => "Palettes",
            Self::Performance => "Performance",
            Self::Breakpoints => "Breakpoints",
            Self::Cheats => "Cheats",
        }
    }

    pub(crate) fn persist_name(self) -> &'static str {
        match self {
            Self::CpuDebug => "CpuDebug",
            Self::ApuViewer => "ApuViewer",
            Self::RomInfo => "RomInfo",
            Self::Disassembler => "Disassembler",
            Self::MemoryViewer => "MemoryViewer",
            Self::TileViewer => "TileViewer",
            Self::TilemapViewer => "TilemapViewer",
            Self::OamViewer => "OamViewer",
            Self::PaletteViewer => "PaletteViewer",
            Self::Performance => "Performance",
            Self::Breakpoints => "Breakpoints",
            Self::Cheats => "Cheats",
        }
    }

    pub(crate) fn from_persist_name(name: &str) -> Option<Self> {
        match name {
            "CpuDebug" => Some(Self::CpuDebug),
            "ApuViewer" => Some(Self::ApuViewer),
            "RomInfo" => Some(Self::RomInfo),
            "Disassembler" => Some(Self::Disassembler),
            "MemoryViewer" => Some(Self::MemoryViewer),
            "TileViewer" => Some(Self::TileViewer),
            "TilemapViewer" => Some(Self::TilemapViewer),
            "OamViewer" => Some(Self::OamViewer),
            "PaletteViewer" => Some(Self::PaletteViewer),
            "Performance" => Some(Self::Performance),
            "Breakpoints" => Some(Self::Breakpoints),
            "Cheats" => Some(Self::Cheats),
            _ => None,
        }
    }
}

pub(crate) fn create_default_dock_state() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![]);
    dock.add_window(vec![DebugTab::CpuDebug]);
    dock
}

pub(crate) fn create_dock_from_saved_tabs(tab_names: &[String]) -> DockState<DebugTab> {
    let tabs: Vec<DebugTab> = tab_names
        .iter()
        .filter_map(|name| DebugTab::from_persist_name(name))
        .collect();
    if tabs.is_empty() {
        return create_default_dock_state();
    }
    let mut dock = DockState::new(vec![]);
    for tab in tabs {
        dock.add_window(vec![tab]);
    }
    dock
}

pub(crate) fn save_open_tabs(dock: &DockState<DebugTab>) -> Vec<String> {
    dock.iter_all_tabs()
        .map(|(_, tab)| tab.persist_name().to_string())
        .collect()
}

pub(crate) fn toggle_dock_tab(dock: &mut DockState<DebugTab>, tab: DebugTab) {
    if let Some(loc) = dock.find_tab(&tab) {
        dock.remove_tab(loc);
    } else {
        dock.add_window(vec![tab]);
    }
}

pub(crate) fn sync_show_flags(
    debug_windows: &mut DebugWindowState,
    dock: &DockState<DebugTab>,
) {
    debug_windows.show_cpu_debug = dock.find_tab(&DebugTab::CpuDebug).is_some();
    debug_windows.show_apu_viewer = dock.find_tab(&DebugTab::ApuViewer).is_some();
    debug_windows.show_rom_info = dock.find_tab(&DebugTab::RomInfo).is_some();
    debug_windows.show_disassembler = dock.find_tab(&DebugTab::Disassembler).is_some();
    debug_windows.show_memory_viewer = dock.find_tab(&DebugTab::MemoryViewer).is_some();
    debug_windows.show_tile_viewer = dock.find_tab(&DebugTab::TileViewer).is_some();
    debug_windows.show_tilemap_viewer = dock.find_tab(&DebugTab::TilemapViewer).is_some();
    debug_windows.show_oam_viewer = dock.find_tab(&DebugTab::OamViewer).is_some();
    debug_windows.show_palette_viewer = dock.find_tab(&DebugTab::PaletteViewer).is_some();
    debug_windows.show_performance = dock.find_tab(&DebugTab::Performance).is_some();
    debug_windows.show_breakpoints_window = dock.find_tab(&DebugTab::Breakpoints).is_some();
    debug_windows.show_cheats = dock.find_tab(&DebugTab::Cheats).is_some();
}

pub(crate) struct DebugTabViewer<'a> {
    pub(crate) debug_info: Option<&'a DebugInfo>,
    pub(crate) viewer_data: Option<&'a DebugViewerData>,
    pub(crate) rom_info_view: Option<&'a RomInfoViewData>,
    pub(crate) disassembly_view: Option<&'a DisassemblyView>,
    pub(crate) memory_page: Option<&'a [(u16, u8)]>,
    pub(crate) window_state: &'a mut DebugWindowState,
    pub(crate) actions: DebugUiActions,
}

impl TabViewer for DebugTabViewer<'_> {
    type Tab = DebugTab;

    fn title(&mut self, tab: &mut DebugTab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut DebugTab) {
        match tab {
            DebugTab::CpuDebug => {
                if let Some(info) = self.debug_info {
                    draw_debug_ui_content(ui, info, self.window_state, &mut self.actions);
                }
            }
            DebugTab::ApuViewer => {
                if let Some(data) = self.viewer_data {
                    if let Some(mutes) = draw_apu_viewer_content(ui, data) {
                        self.actions.apu_channel_mutes = Some(mutes);
                    }
                }
            }
            DebugTab::RomInfo => {
                if let Some(info) = self.rom_info_view {
                    draw_rom_info_content(ui, info);
                }
            }
            DebugTab::Disassembler => {
                if let Some(view) = self.disassembly_view {
                    let toggles = draw_disassembler_content(ui, view);
                    self.actions.toggle_breakpoints.extend(toggles);
                }
            }
            DebugTab::MemoryViewer => {
                if let Some(page) = self.memory_page {
                    let writes = draw_memory_viewer_content(ui, self.window_state, page);
                    self.actions.memory_writes.extend(writes);
                }
            }
            DebugTab::TileViewer => {
                if let Some(data) = self.viewer_data {
                    draw_tile_viewer_content(
                        ui,
                        &data.vram,
                        data.ppu.bgp,
                        data.cgb_mode,
                        &data.bg_palette_ram,
                        &data.obj_palette_ram,
                        self.window_state,
                    );
                }
            }
            DebugTab::TilemapViewer => {
                if let Some(data) = self.viewer_data {
                    draw_tilemap_viewer_content(
                        ui,
                        &data.vram,
                        data.ppu,
                        data.cgb_mode,
                        &data.bg_palette_ram,
                        self.window_state,
                    );
                }
            }
            DebugTab::OamViewer => {
                if let Some(data) = self.viewer_data {
                    draw_oam_viewer_content(ui, &data.oam);
                }
            }
            DebugTab::PaletteViewer => {
                if let Some(data) = self.viewer_data {
                    draw_palette_viewer_content(
                        ui,
                        data.ppu.bgp,
                        data.ppu.obp0,
                        data.ppu.obp1,
                        data.cgb_mode,
                        &data.bg_palette_ram,
                        &data.obj_palette_ram,
                    );
                }
            }
            DebugTab::Performance => {
                if let Some(info) = self.debug_info {
                    draw_performance_content(
                        ui,
                        info,
                        &mut self.window_state.perf_history,
                    );
                }
            }
            DebugTab::Breakpoints => {
                if let Some(info) = self.debug_info {
                    draw_breakpoints_content(
                        ui,
                        info,
                        self.window_state,
                        &mut self.actions,
                    );
                }
            }
            DebugTab::Cheats => {
                draw_cheats_content(ui, self.window_state);
            }
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, true]
    }
}
