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
    if ui
        .checkbox(&mut debug_windows.layer_enable_bg, "Background")
        .changed()
    {
        debug_windows.gba_layer_enable_bg = [debug_windows.layer_enable_bg; 4];
    }
    let mut gba_bg_changed = false;
    ui.add_enabled_ui(debug_windows.layer_enable_bg, |ui| {
        ui.horizontal(|ui| {
            ui.label("GBA");
            for bg in 0..4 {
                gba_bg_changed |= ui
                    .checkbox(
                        &mut debug_windows.gba_layer_enable_bg[bg],
                        format!("BG{bg}"),
                    )
                    .changed();
            }
        });
    });
    if gba_bg_changed {
        debug_windows.layer_enable_bg = debug_windows
            .gba_layer_enable_bg
            .iter()
            .any(|&enabled| enabled);
    }
    ui.checkbox(&mut debug_windows.layer_enable_window, "Window");
    ui.checkbox(&mut debug_windows.layer_enable_sprites, "Sprites");
}
