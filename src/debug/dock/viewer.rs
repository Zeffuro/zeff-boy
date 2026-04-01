use egui_dock::{TabViewer, widgets::tab_viewer::OnCloseResponse};

use super::super::apu_viewer::draw_apu_viewer_content;
use super::super::breakpoints_window::draw_breakpoints_content;
use super::super::cheats_window::draw_cheats_content;
use super::super::disasm_window::draw_disassembler_content;
use super::super::input_viewer::draw_input_viewer_content;
use super::super::memory_viewer::draw_memory_viewer_content;
use super::super::mods_window::draw_mods_content;
use super::super::nes_tile_viewer::draw_nes_tile_viewer_content;
use super::super::nes_tilemap_viewer::draw_nes_tilemap_viewer_content;
use super::super::oam_viewer::draw_oam_viewer_content;
use super::super::palette_viewer::draw_palette_viewer_content;
use super::super::perf_monitor::draw_performance_content;
use super::super::rom_info::draw_rom_info_content;
use super::super::rom_viewer::draw_rom_viewer_content;
use super::super::tile_viewer::draw_tile_viewer_content;
use super::super::tilemap_viewer::draw_tilemap_viewer_content;
use super::super::types::{
    ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, InputDebugInfo, OamDebugInfo,
    PaletteDebugInfo, PerfInfo, RomDebugInfo,
};
use super::super::ui::draw_cpu_debug_content;
use super::super::{DebugUiActions, DebugWindowState, DisassemblyView};
use crate::graphics::AspectRatioMode;

use super::tabs::DebugTab;

pub(crate) struct DebugTabViewer<'a> {
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
                if let Some(info) = self.cpu_debug {
                    draw_cpu_debug_content(ui, info, &mut self.actions);
                }
            }
            DebugTab::InputViewer => {
                if let Some(info) = self.input_debug {
                    draw_input_viewer_content(ui, info);
                }
            }
            DebugTab::ApuViewer => {
                if let Some(data) = self.apu_debug
                    && let Some(mutes) = draw_apu_viewer_content(ui, data)
                {
                    self.actions.apu_channel_mutes = Some(mutes);
                }
            }
            DebugTab::RomInfo => {
                if let Some(info) = self.rom_debug {
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
                if let Some(ConsoleGraphicsData::Gb(data)) = self.graphics_data {
                    draw_tile_viewer_content(ui, data, data.ppu.bgp, &mut self.window_state.tiles);
                } else if let Some(ConsoleGraphicsData::Nes(data)) = self.graphics_data {
                    draw_nes_tile_viewer_content(ui, data, &mut self.window_state.tiles);
                }
            }
            DebugTab::TilemapViewer => {
                if let Some(ConsoleGraphicsData::Gb(data)) = self.graphics_data {
                    draw_tilemap_viewer_content(ui, data, &mut self.window_state.tilemap);
                } else if let Some(ConsoleGraphicsData::Nes(data)) = self.graphics_data {
                    draw_nes_tilemap_viewer_content(ui, data, &mut self.window_state.tilemap);
                }
            }
            DebugTab::OamViewer => {
                if let Some(info) = self.oam_debug {
                    draw_oam_viewer_content(ui, info);
                }
            }
            DebugTab::PaletteViewer => {
                if let Some(info) = self.palette_debug {
                    draw_palette_viewer_content(ui, info);
                }
            }
            DebugTab::Performance => {
                if let Some(info) = self.perf_info {
                    draw_performance_content(ui, info, &mut self.window_state.perf_history);
                }
            }
            DebugTab::Breakpoints => {
                if let Some(info) = self.cpu_debug {
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
            DebugTab::Mods => {
                draw_mods_content(ui, &mut self.window_state.mod_state);
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
