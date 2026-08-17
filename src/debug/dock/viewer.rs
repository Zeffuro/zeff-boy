use egui_dock::{TabViewer, widgets::tab_viewer::OnCloseResponse};

use super::super::apu_viewer::draw_apu_viewer_content;
use super::super::breakpoints_window::draw_breakpoints_content;
use super::super::call_stack::draw_call_stack_content;
use super::super::cheats_window::draw_cheats_content;
use super::super::console::{DebugConsoleContext, DebugConsoleViews, draw_debug_console_content};
use super::super::disasm_window::draw_disassembler_content;
use super::super::execution_history::draw_execution_history_content;
use super::super::gba_tile_viewer::draw_gba_tile_viewer_content;
use super::super::gba_tilemap_viewer::draw_gba_tilemap_viewer_content;
use super::super::hardware_io::draw_hardware_io_content;
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
use super::super::sega8_tile_viewer::draw_sega8_tile_viewer_content;
use super::super::source_viewer::draw_source_viewer_content;
use super::super::symbol_browser::{SymbolBrowserViews, draw_symbol_browser_content};
use super::super::tile_viewer::draw_tile_viewer_content;
use super::super::tilemap_viewer::draw_tilemap_viewer_content;
use super::super::types::{ConsoleGraphicsData, DebugDataRefs};
use super::super::ui::draw_cpu_debug_content;
use super::super::{DebugUiActions, DebugWindowState};
use crate::graphics::AspectRatioMode;

use super::tabs::DebugTab;

pub(crate) struct DebugTabViewer<'a> {
    pub(crate) data: DebugDataRefs<'a>,
    pub(crate) window_state: &'a mut DebugWindowState,
    pub(crate) actions: DebugUiActions,
    pub(crate) game_texture_id: Option<egui::TextureId>,
    pub(crate) game_native_size: (u32, u32),
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
                    let game_w = self.game_native_size.0.max(1) as f32;
                    let game_h = self.game_native_size.1.max(1) as f32;

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
                        (w * ppp).round().max(game_w) as u32,
                        (h * ppp).round().max(game_h) as u32,
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
                if let Some(info) = self.data.cpu_debug {
                    draw_cpu_debug_content(
                        ui,
                        info,
                        &mut self.window_state.cpu_view,
                        &mut self.actions,
                    );
                }
            }
            DebugTab::HardwareIo => {
                if let Some(info) = self.data.cpu_debug {
                    draw_hardware_io_content(
                        ui,
                        info,
                        &mut self.window_state.hardware_view,
                        &mut self.actions,
                    );
                }
            }
            DebugTab::InputViewer => {
                if let Some(info) = self.data.input_debug {
                    draw_input_viewer_content(ui, info);
                }
            }
            DebugTab::ApuViewer => {
                if let Some(data) = self.data.apu_debug
                    && let Some(mutes) = draw_apu_viewer_content(ui, data)
                {
                    self.actions.apu_channel_mutes = Some(mutes);
                }
            }
            DebugTab::RomInfo => {
                if let Some(info) = self.data.rom_debug {
                    draw_rom_info_content(ui, info);
                }
            }
            DebugTab::Disassembler => {
                if let Some(view) = self.data.disassembly_view {
                    let disasm_actions = draw_disassembler_content(ui, view);
                    self.actions
                        .toggle_breakpoints
                        .extend(disasm_actions.toggle_breakpoints);
                    self.actions
                        .toggle_rom_breakpoints
                        .extend(disasm_actions.toggle_rom_breakpoints);
                    if disasm_actions.add_one_shot_breakpoint.is_some() {
                        self.actions.add_one_shot_breakpoint =
                            disasm_actions.add_one_shot_breakpoint;
                    }
                    self.actions.step_requested |= disasm_actions.step_requested;
                    self.actions.continue_requested |= disasm_actions.continue_requested;
                    self.actions.backstep_requested |= disasm_actions.backstep_requested;
                    self.actions.follow_disasm_pc |= disasm_actions.follow_pc_requested;
                    self.actions.disasm_back |= disasm_actions.back_requested;
                    self.actions.disasm_forward |= disasm_actions.forward_requested;
                    if disasm_actions.disasm_target.is_some() {
                        self.actions.disasm_target = disasm_actions.disasm_target;
                    }
                }
            }
            DebugTab::MemoryViewer => {
                if let Some(page) = self.data.memory_page {
                    let writes = draw_memory_viewer_content(
                        ui,
                        &mut self.window_state.memory,
                        page,
                        self.data.symbols,
                        self.data.cpu_debug,
                        &mut self.actions,
                    );
                    self.actions.memory_writes.extend(writes);
                }
            }
            DebugTab::TileViewer => {
                if let Some(ConsoleGraphicsData::Gb(data)) = self.data.graphics_data {
                    draw_tile_viewer_content(ui, data, data.ppu.bgp, &mut self.window_state.tiles);
                } else if let Some(ConsoleGraphicsData::Gba(data)) = self.data.graphics_data {
                    draw_gba_tile_viewer_content(
                        ui,
                        data,
                        &mut self.window_state.tiles,
                        &mut self.actions,
                    );
                } else if let Some(ConsoleGraphicsData::Nes(data)) = self.data.graphics_data {
                    draw_nes_tile_viewer_content(ui, data, &mut self.window_state.tiles);
                } else if let Some(ConsoleGraphicsData::Sega8(data)) = self.data.graphics_data {
                    draw_sega8_tile_viewer_content(ui, data, &mut self.window_state.tiles);
                }
            }
            DebugTab::TilemapViewer => {
                if let Some(ConsoleGraphicsData::Gb(data)) = self.data.graphics_data {
                    draw_tilemap_viewer_content(
                        ui,
                        data,
                        &mut self.window_state.tilemap,
                        &mut self.window_state.tiles,
                        &mut self.actions,
                    );
                } else if let Some(ConsoleGraphicsData::Gba(data)) = self.data.graphics_data {
                    draw_gba_tilemap_viewer_content(ui, data, &mut self.window_state.tilemap);
                } else if let Some(ConsoleGraphicsData::Nes(data)) = self.data.graphics_data {
                    draw_nes_tilemap_viewer_content(ui, data, &mut self.window_state.tilemap);
                }
            }
            DebugTab::OamViewer => {
                if let Some(info) = self.data.oam_debug {
                    draw_oam_viewer_content(
                        ui,
                        info,
                        self.data.graphics_data,
                        &mut self.window_state.oam,
                        &mut self.window_state.tiles,
                        &mut self.actions,
                    );
                }
            }
            DebugTab::PaletteViewer => {
                if let Some(info) = self.data.palette_debug {
                    draw_palette_viewer_content(ui, info);
                }
            }
            DebugTab::Performance => {
                if let Some(info) = self.data.perf_info {
                    draw_performance_content(ui, info, &mut self.window_state.perf_history);
                }
            }
            DebugTab::Breakpoints => {
                if let Some(info) = self.data.cpu_debug {
                    draw_breakpoints_content(
                        ui,
                        info,
                        self.data.symbols,
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
                if let Some(page) = self.data.rom_page {
                    draw_rom_viewer_content(
                        ui,
                        &mut self.window_state.rom_viewer,
                        page,
                        self.data.rom_size,
                        self.data.symbols,
                    );
                }
            }
            DebugTab::SourceViewer => {
                draw_source_viewer_content(
                    ui,
                    &mut self.window_state.source_viewer,
                    self.data.symbols,
                    self.data.disassembly_view,
                    self.data.cpu_debug,
                    &mut self.actions,
                );
            }
            DebugTab::ExecutionHistory => {
                if let Some(info) = self.data.cpu_debug {
                    draw_execution_history_content(ui, info, self.data.symbols, &mut self.actions);
                }
            }
            DebugTab::CallStack => {
                if let Some(info) = self.data.cpu_debug {
                    draw_call_stack_content(ui, info, self.data.symbols, &mut self.actions);
                }
            }
            DebugTab::SymbolBrowser => {
                draw_symbol_browser_content(
                    ui,
                    self.data.symbols,
                    self.data.cpu_debug,
                    SymbolBrowserViews {
                        state: &mut self.window_state.symbol_browser,
                        memory: &mut self.window_state.memory,
                        rom: &mut self.window_state.rom_viewer,
                    },
                    &mut self.actions,
                );
            }
            DebugTab::Console => {
                draw_debug_console_content(
                    ui,
                    &mut self.window_state.console,
                    DebugConsoleContext {
                        symbols: self.data.symbols,
                        cpu_debug: self.data.cpu_debug,
                        rom_debug: self.data.rom_debug,
                        disassembly: self.data.disassembly_view,
                        memory_page: self.data.memory_page,
                        rom_page: self.data.rom_page,
                    },
                    DebugConsoleViews {
                        memory: &mut self.window_state.memory,
                        rom: &mut self.window_state.rom_viewer,
                    },
                    &mut self.actions,
                );
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
            DebugTab::GameView | DebugTab::Console => [false, false],
            _ => [false, true],
        }
    }
}
