use crate::debug::dock::{DebugTab, toggle_dock_tab};
use egui_dock::DockState;

pub(super) fn draw(ui: &mut egui::Ui, dock_state: &mut DockState<DebugTab>) {
    if !crate::debug::is_tab_open(dock_state, crate::debug::DebugTab::GameView) {
        if ui.button("Show Game View").clicked() {
            crate::debug::ensure_game_view_tab(dock_state);
            ui.close();
        }
        ui.separator();
    }
    if ui.button("CPU / Debug").clicked() {
        toggle_dock_tab(dock_state, DebugTab::CpuDebug);
        ui.close();
    }
    if ui.button("Disassembler").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Disassembler);
        ui.close();
    }
    if ui.button("Breakpoints").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Breakpoints);
        ui.close();
    }
    ui.separator();
    if ui.button("Memory Viewer").clicked() {
        toggle_dock_tab(dock_state, DebugTab::MemoryViewer);
        ui.close();
    }
    if ui.button("ROM Viewer").clicked() {
        toggle_dock_tab(dock_state, DebugTab::RomViewer);
        ui.close();
    }
    if ui.button("ROM Info").clicked() {
        toggle_dock_tab(dock_state, DebugTab::RomInfo);
        ui.close();
    }
    ui.separator();
    ui.menu_button("Graphics", |ui| {
        if ui.button("Tile Data").clicked() {
            toggle_dock_tab(dock_state, DebugTab::TileViewer);
            ui.close();
        }
        if ui.button("Tile Map").clicked() {
            toggle_dock_tab(dock_state, DebugTab::TilemapViewer);
            ui.close();
        }
        if ui.button("OAM / Sprites").clicked() {
            toggle_dock_tab(dock_state, DebugTab::OamViewer);
            ui.close();
        }
        if ui.button("Palettes").clicked() {
            toggle_dock_tab(dock_state, DebugTab::PaletteViewer);
            ui.close();
        }
    });
    if ui.button("APU / Sound").clicked() {
        toggle_dock_tab(dock_state, DebugTab::ApuViewer);
        ui.close();
    }
    if ui.button("Input").clicked() {
        toggle_dock_tab(dock_state, DebugTab::InputViewer);
        ui.close();
    }
    if ui.button("Performance").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Performance);
        ui.close();
    }
    ui.separator();
    if ui.button("Reset Layout (Floating)").clicked() {
        *dock_state = crate::debug::create_default_dock_state();
        ui.close();
    }
    if ui.button("Reset Layout (IDE)").clicked() {
        *dock_state = crate::debug::create_ide_dock_state();
        ui.close();
    }
}
