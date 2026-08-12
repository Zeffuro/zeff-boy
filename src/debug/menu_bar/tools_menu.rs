use super::MenuAction;
use crate::debug::DebugWindowState;
use crate::debug::dock::{DebugTab, toggle_dock_tab};
use crate::emu_backend::ActiveSystem;
use egui_dock::DockState;

pub(super) fn draw(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    dock_state: &mut DockState<DebugTab>,
    debug_windows: &mut DebugWindowState,
    active_system: ActiveSystem,
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
    ui.label("Link Cable");
    let gb_link_enabled = active_system == ActiveSystem::GameBoy;
    #[cfg(target_arch = "wasm32")]
    ui.label("TCP link is native-only");
    if ui
        .add_enabled(gb_link_enabled, egui::Button::new("Host TCP Link"))
        .on_hover_text(
            "Open this after loading the first GB/GBC ROM, then Join from another app instance",
        )
        .clicked()
    {
        actions.push(MenuAction::HostTcpLink);
        ui.close();
    }
    if ui
        .add_enabled(gb_link_enabled, egui::Button::new("Join TCP Link"))
        .on_hover_text("Join a localhost link hosted by another app instance")
        .clicked()
    {
        actions.push(MenuAction::JoinTcpLink);
        ui.close();
    }
    if ui.button("Disconnect Link").clicked() {
        actions.push(MenuAction::DisconnectLink);
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
