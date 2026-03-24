use egui_dock::{DockState, TabViewer, widgets::tab_viewer::OnCloseResponse};

use super::apu_viewer::draw_apu_viewer_content;
use super::breakpoints_window::draw_breakpoints_content;
use super::cheats_window::draw_cheats_content;
use super::disasm_window::draw_disassembler_content;
use super::input_viewer::draw_input_viewer_content;
use super::memory_viewer::draw_memory_viewer_content;
use super::oam_viewer::draw_oam_viewer_content;
use super::palette_viewer::draw_palette_viewer_content;
use super::perf_monitor::draw_performance_content;
use super::rom_info::draw_rom_info_content;
use super::rom_viewer::draw_rom_viewer_content;
use super::tile_viewer::draw_tile_viewer_content;
use super::tilemap_viewer::draw_tilemap_viewer_content;
use super::ui::draw_debug_ui_content;
use super::{
    DebugInfo, DebugUiActions, DebugViewerData, DebugWindowState, DisassemblyView, RomInfoViewData,
};
use crate::graphics::AspectRatioMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DebugTab {
    GameView,
    CpuDebug,
    InputViewer,
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
    RomViewer,
}

const TAB_META: &[(DebugTab, &str, &str)] = &[
    (DebugTab::GameView, "Game", "GameView"),
    (DebugTab::CpuDebug, "CPU / Debug", "CpuDebug"),
    (DebugTab::InputViewer, "Input", "InputViewer"),
    (DebugTab::ApuViewer, "APU / Sound", "ApuViewer"),
    (DebugTab::RomInfo, "ROM Info", "RomInfo"),
    (DebugTab::Disassembler, "Disassembler", "Disassembler"),
    (DebugTab::MemoryViewer, "Memory Viewer", "MemoryViewer"),
    (DebugTab::RomViewer, "ROM Viewer", "RomViewer"),
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
        TAB_META
            .iter()
            .find(|(t, _, _)| *t == self)
            .map(|(_, title, _)| *title)
            .unwrap_or("?")
    }

    pub(crate) fn persist_name(self) -> &'static str {
        TAB_META
            .iter()
            .find(|(t, _, _)| *t == self)
            .map(|(_, _, name)| *name)
            .unwrap_or("?")
    }

    pub(crate) fn from_persist_name(name: &str) -> Option<Self> {
        TAB_META
            .iter()
            .find(|(_, _, n)| *n == name)
            .map(|(tab, _, _)| *tab)
    }
}

pub(crate) fn create_default_dock_state() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![]);
    dock.add_window(vec![DebugTab::CpuDebug]);
    dock
}

pub(crate) fn create_ide_dock_state() -> DockState<DebugTab> {
    // Central area: Game view
    let mut dock = DockState::new(vec![DebugTab::GameView]);
    let tree = dock.main_surface_mut();

    // Left panel: CPU debug + Performance + APU + Input
    let [_center, _left] = tree.split_left(
        egui_dock::NodeIndex::root(),
        0.25,
        vec![
            DebugTab::CpuDebug,
            DebugTab::Performance,
            DebugTab::ApuViewer,
            DebugTab::InputViewer,
        ],
    );

    // Right panel: Disassembler + Memory + ROM Viewer
    let [_center2, right] = tree.split_right(
        egui_dock::NodeIndex::root(),
        0.65,
        vec![
            DebugTab::Disassembler,
            DebugTab::MemoryViewer,
            DebugTab::RomViewer,
        ],
    );

    // Bottom-right: Breakpoints + Cheats
    let [_right_top, _right_bottom] =
        tree.split_below(right, 0.65, vec![DebugTab::Breakpoints, DebugTab::Cheats]);

    // Below game view: Graphics viewers grouped together
    let [_center3, _bottom] = tree.split_below(
        egui_dock::NodeIndex::root(),
        0.6,
        vec![
            DebugTab::TileViewer,
            DebugTab::TilemapViewer,
            DebugTab::OamViewer,
            DebugTab::PaletteViewer,
        ],
    );

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

    let has_game_view = tabs.contains(&DebugTab::GameView);
    let non_game_tabs: Vec<DebugTab> = tabs
        .iter()
        .copied()
        .filter(|t| *t != DebugTab::GameView)
        .collect();

    if has_game_view {
        let mut dock = DockState::new(vec![DebugTab::GameView]);
        if !non_game_tabs.is_empty() {
            dock.add_window(non_game_tabs);
        }
        dock
    } else {
        let mut dock = DockState::new(vec![]);
        if !non_game_tabs.is_empty() {
            dock.add_window(non_game_tabs);
        }
        dock
    }
}

pub(crate) fn save_open_tabs(dock: &DockState<DebugTab>) -> Vec<String> {
    dock.iter_all_tabs()
        .map(|(_, tab)| tab.persist_name().to_string())
        .collect()
}

pub(crate) fn has_game_view_tab(dock: &DockState<DebugTab>) -> bool {
    dock.iter_all_tabs()
        .any(|(_, tab)| *tab == DebugTab::GameView)
}

/// Re-add the GameView tab if it was lost (safety net).
pub(crate) fn ensure_game_view_tab(dock: &mut DockState<DebugTab>) {
    if !has_game_view_tab(dock) {
        dock.main_surface_mut()
            .push_to_focused_leaf(DebugTab::GameView);
    }
}

pub(crate) fn toggle_dock_tab(dock: &mut DockState<DebugTab>, tab: DebugTab) {
    if let Some(loc) = dock.find_tab(&tab) {
        dock.remove_tab(loc);
    } else {
        dock.add_window(vec![tab]);
    }
}

pub(crate) fn sync_show_flags(debug_windows: &mut DebugWindowState, dock: &DockState<DebugTab>) {
    let open: std::collections::HashSet<DebugTab> =
        dock.iter_all_tabs().map(|(_, tab)| *tab).collect();
    debug_windows.show_cpu_debug = open.contains(&DebugTab::CpuDebug);
    debug_windows.show_input_viewer = open.contains(&DebugTab::InputViewer);
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
    debug_windows.show_rom_viewer = open.contains(&DebugTab::RomViewer);
}

pub(crate) struct DebugTabViewer<'a> {
    pub(crate) debug_info: Option<&'a DebugInfo>,
    pub(crate) viewer_data: Option<&'a DebugViewerData>,
    pub(crate) rom_info_view: Option<&'a RomInfoViewData>,
    pub(crate) disassembly_view: Option<&'a DisassemblyView>,
    pub(crate) memory_page: Option<&'a [(u16, u8)]>,
    pub(crate) rom_page: Option<&'a [(u32, u8)]>,
    pub(crate) rom_size: u32,
    pub(crate) window_state: &'a mut DebugWindowState,
    pub(crate) actions: DebugUiActions,
    pub(crate) game_texture_id: Option<egui::TextureId>,
    pub(crate) aspect_ratio_mode: AspectRatioMode,
    pub(crate) game_view_pixel_size: Option<(u32, u32)>,
}

impl TabViewer for DebugTabViewer<'_> {
    type Tab = DebugTab;

    fn title(&mut self, tab: &mut DebugTab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut DebugTab) {
        match tab {
            DebugTab::GameView => {
                if let Some(tex_id) = self.game_texture_id {
                    let available = ui.available_size();
                    let game_w = 160.0_f32;
                    let game_h = 144.0_f32;

                    let (w, h) = match self.aspect_ratio_mode {
                        AspectRatioMode::Stretch => (available.x, available.y),
                        AspectRatioMode::KeepAspect => {
                            let scale = (available.x / game_w).min(available.y / game_h).max(1.0);
                            (game_w * scale, game_h * scale)
                        }
                        AspectRatioMode::IntegerScale => {
                            let scale_x = (available.x / game_w).floor().max(1.0);
                            let scale_y = (available.y / game_h).floor().max(1.0);
                            let scale = scale_x.min(scale_y);
                            (game_w * scale, game_h * scale)
                        }
                    };

                    let ppp = ui.ctx().pixels_per_point();
                    self.game_view_pixel_size = Some((
                        (w * ppp).round().max(160.0) as u32,
                        (h * ppp).round().max(144.0) as u32,
                    ));

                    let rect = ui.available_rect_before_wrap();
                    ui.painter()
                        .rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 20, 30));

                    let offset_x = rect.min.x + (available.x - w) / 2.0;
                    let offset_y = rect.min.y + (available.y - h) / 2.0;
                    let image_rect =
                        egui::Rect::from_min_size(egui::pos2(offset_x, offset_y), egui::vec2(w, h));
                    let image =
                        egui::Image::new(egui::load::SizedTexture::new(tex_id, egui::vec2(w, h)));
                    ui.put(image_rect, image);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.heading("No game loaded");
                    });
                }
            }
            DebugTab::CpuDebug => {
                if let Some(info) = self.debug_info {
                    draw_debug_ui_content(ui, info, &mut self.actions);
                }
            }
            DebugTab::InputViewer => {
                if let Some(info) = self.debug_info {
                    draw_input_viewer_content(ui, info);
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
                    let disasm_actions = draw_disassembler_content(ui, view);
                    self.actions
                        .toggle_breakpoints
                        .extend(disasm_actions.toggle_breakpoints);
                    self.actions.step_requested |= disasm_actions.step_requested;
                    self.actions.continue_requested |= disasm_actions.continue_requested;
                    self.actions.backstep_requested |= disasm_actions.backstep_requested;
                }
            }
            DebugTab::MemoryViewer => {
                if let Some(page) = self.memory_page {
                    let writes =
                        draw_memory_viewer_content(ui, &mut self.window_state.memory, page);
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
                        data.color_correction,
                        data.color_correction_matrix,
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
                        data.color_correction,
                        data.color_correction_matrix,
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
                        data.color_correction,
                        data.color_correction_matrix,
                    );
                }
            }
            DebugTab::Performance => {
                if let Some(info) = self.debug_info {
                    draw_performance_content(ui, info, &mut self.window_state.perf_history);
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
            DebugTab::RomViewer => {
                if let Some(page) = self.rom_page {
                    draw_rom_viewer_content(
                        ui,
                        &mut self.window_state.rom_viewer,
                        page,
                        self.rom_size,
                    );
                }
            }
        }
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        if *tab == DebugTab::GameView {
            OnCloseResponse::Ignore
        } else {
            OnCloseResponse::Close
        }
    }

    fn closeable(&mut self, tab: &mut Self::Tab) -> bool {
        *tab != DebugTab::GameView
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        match tab {
            DebugTab::GameView => [false, false],
            _ => [false, true],
        }
    }
}
