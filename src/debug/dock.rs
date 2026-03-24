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

const TAB_META: &[(DebugTab, &str, &str)] = &[
    (DebugTab::CpuDebug, "CPU / Debug", "CpuDebug"),
    (DebugTab::ApuViewer, "APU / Sound", "ApuViewer"),
    (DebugTab::RomInfo, "ROM Info", "RomInfo"),
    (DebugTab::Disassembler, "Disassembler", "Disassembler"),
    (DebugTab::MemoryViewer, "Memory Viewer", "MemoryViewer"),
    (DebugTab::TileViewer, "Tile Data", "TileViewer"),
    (DebugTab::TilemapViewer, "Tile Map", "TilemapViewer"),
    (DebugTab::OamViewer, "OAM / Sprites", "OamViewer"),
    (DebugTab::PaletteViewer, "Palettes", "PaletteViewer"),
    (DebugTab::Performance, "Performance", "Performance"),
    (DebugTab::Breakpoints, "Breakpoints", "Breakpoints"),
    (DebugTab::Cheats, "Cheats", "Cheats"),
];

impl DebugTab {
    fn title(self) -> &'static str {
        TAB_META.iter().find(|(t, _, _)| *t == self).map(|(_, title, _)| *title).unwrap_or("?")
    }

    pub(crate) fn persist_name(self) -> &'static str {
        TAB_META.iter().find(|(t, _, _)| *t == self).map(|(_, _, name)| *name).unwrap_or("?")
    }

    pub(crate) fn from_persist_name(name: &str) -> Option<Self> {
        TAB_META.iter().find(|(_, _, n)| *n == name).map(|(tab, _, _)| *tab)
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
    let open: std::collections::HashSet<DebugTab> =
        dock.iter_all_tabs().map(|(_, tab)| *tab).collect();
    debug_windows.show_cpu_debug = open.contains(&DebugTab::CpuDebug);
    debug_windows.show_apu_viewer = open.contains(&DebugTab::ApuViewer);
    debug_windows.show_rom_info = open.contains(&DebugTab::RomInfo);
    debug_windows.show_disassembler = open.contains(&DebugTab::Disassembler);
    debug_windows.show_memory_viewer = open.contains(&DebugTab::MemoryViewer);
    debug_windows.show_tile_viewer = open.contains(&DebugTab::TileViewer);
    debug_windows.show_tilemap_viewer = open.contains(&DebugTab::TilemapViewer);
    debug_windows.show_oam_viewer = open.contains(&DebugTab::OamViewer);
    debug_windows.show_palette_viewer = open.contains(&DebugTab::PaletteViewer);
    debug_windows.show_performance = open.contains(&DebugTab::Performance);
    debug_windows.show_breakpoints_window = open.contains(&DebugTab::Breakpoints);
    debug_windows.show_cheats = open.contains(&DebugTab::Cheats);
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
                    let writes = draw_memory_viewer_content(ui, &mut self.window_state.memory, page);
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
                        &mut self.window_state.tiles,
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
                        &mut self.window_state.tilemap,
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
                        &mut self.window_state.bp,
                        &mut self.actions,
                    );
                }
            }
            DebugTab::Cheats => {
                draw_cheats_content(ui, &mut self.window_state.cheat);
            }
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, true]
    }
}
