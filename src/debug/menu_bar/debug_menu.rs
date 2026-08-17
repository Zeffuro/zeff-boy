use super::MenuAction;
use crate::debug::dock::{DebugTab, toggle_dock_tab};
use crate::debug::ui_helpers::EnumLabel;
use crate::settings::DebugPresentation;
use egui_dock::DockState;

pub(super) fn draw(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    dock_state: &mut DockState<DebugTab>,
    external_debugger: bool,
    debugger_window_open: bool,
    presentation: DebugPresentation,
) {
    ui.menu_button("Presentation", |ui| {
        for &mode in DebugPresentation::all_variants() {
            if ui
                .selectable_label(mode == presentation, mode.label())
                .clicked()
            {
                actions.push(MenuAction::SetDebugPresentation(mode));
                ui.close();
            }
        }
    });
    if presentation != DebugPresentation::Floating {
        ui.menu_button("Workspace", |ui| {
            for preset in crate::debug::DebugWorkspacePreset::ALL {
                if ui.button(preset.label()).clicked() {
                    *dock_state = crate::debug::create_workspace_dock_state(presentation, preset);
                    open_external_debugger(actions, external_debugger);
                    ui.close();
                }
            }
        });
    }
    ui.separator();
    if external_debugger && !debugger_window_open {
        if ui.button("Open Debugger Window").clicked() {
            actions.push(MenuAction::OpenDebuggerWindow);
            ui.close();
        }
        ui.separator();
    }
    if !external_debugger
        && !crate::debug::is_tab_open(dock_state, crate::debug::DebugTab::GameView)
    {
        if ui.button("Show Game View").clicked() {
            crate::debug::ensure_game_view_tab(dock_state);
            ui.close();
        }
        ui.separator();
    }
    if ui.button("CPU").clicked() {
        toggle_dock_tab(dock_state, DebugTab::CpuDebug);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Hardware / I/O").clicked() {
        toggle_dock_tab(dock_state, DebugTab::HardwareIo);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Disassembler").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Disassembler);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Breakpoints").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Breakpoints);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Execution History").clicked() {
        toggle_dock_tab(dock_state, DebugTab::ExecutionHistory);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Call Stack").clicked() {
        toggle_dock_tab(dock_state, DebugTab::CallStack);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    ui.separator();
    if ui.button("Memory Viewer").clicked() {
        toggle_dock_tab(dock_state, DebugTab::MemoryViewer);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("ROM Viewer").clicked() {
        toggle_dock_tab(dock_state, DebugTab::RomViewer);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("ROM Info").clicked() {
        toggle_dock_tab(dock_state, DebugTab::RomInfo);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Symbols").clicked() {
        toggle_dock_tab(dock_state, DebugTab::SymbolBrowser);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    #[cfg(not(target_arch = "wasm32"))]
    if ui.button("Load Symbol File...").clicked() {
        actions.push(MenuAction::LoadSymbolFile);
        ui.close();
    }
    ui.separator();
    ui.menu_button("Graphics", |ui| {
        if ui.button("Tile Data").clicked() {
            toggle_dock_tab(dock_state, DebugTab::TileViewer);
            open_external_debugger(actions, external_debugger);
            ui.close();
        }
        if ui.button("Tile Map").clicked() {
            toggle_dock_tab(dock_state, DebugTab::TilemapViewer);
            open_external_debugger(actions, external_debugger);
            ui.close();
        }
        if ui.button("OAM / Sprites").clicked() {
            toggle_dock_tab(dock_state, DebugTab::OamViewer);
            open_external_debugger(actions, external_debugger);
            ui.close();
        }
        if ui.button("Palettes").clicked() {
            toggle_dock_tab(dock_state, DebugTab::PaletteViewer);
            open_external_debugger(actions, external_debugger);
            ui.close();
        }
    });
    if ui.button("APU / Sound").clicked() {
        toggle_dock_tab(dock_state, DebugTab::ApuViewer);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Input").clicked() {
        toggle_dock_tab(dock_state, DebugTab::InputViewer);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    if ui.button("Performance").clicked() {
        toggle_dock_tab(dock_state, DebugTab::Performance);
        open_external_debugger(actions, external_debugger);
        ui.close();
    }
    ui.separator();
    if external_debugger {
        if ui.button("Reset Debugger Layout").clicked() {
            *dock_state = crate::debug::create_debugger_dock_state();
            open_external_debugger(actions, true);
            ui.close();
        }
    } else {
        if ui.button("Reset Layout (Floating)").clicked() {
            *dock_state = crate::debug::create_default_dock_state();
            ui.close();
        }
        if ui.button("Reset Layout (IDE)").clicked() {
            *dock_state = crate::debug::create_ide_dock_state();
            ui.close();
        }
    }
}

fn open_external_debugger(actions: &mut Vec<MenuAction>, external: bool) {
    if external {
        actions.push(MenuAction::OpenDebuggerWindow);
    }
}
