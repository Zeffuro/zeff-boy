use crate::debug::DebugWindowState;
use crate::debug::dock::{DebugTab, toggle_dock_tab};
use egui_dock::DockState;

pub(super) fn draw(
    ui: &mut egui::Ui,
    dock_state: &mut DockState<DebugTab>,
    debug_windows: &mut DebugWindowState,
) {
    if ui.button("Cheats").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Cheats);
        ui.close();
    }
    if ui.button("Mods").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Mods);
        ui.close();
    }
    ui.separator();
    ui.label("PPU Layers");
    ui.checkbox(&mut debug_windows.layer_enable_bg, "Background");
    ui.checkbox(&mut debug_windows.layer_enable_window, "Window");
    ui.checkbox(&mut debug_windows.layer_enable_sprites, "Sprites");
}

