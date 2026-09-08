use crate::settings::Settings;

use super::{
    layout::{checkbox, enum_combo, helper, row},
    search::{self, SettingId as Id},
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    helper(
        ui,
        "Appearance and editing tools for inspecting a running game.",
    );
    ui.add_space(14.0);
    row(ui, Id::DebuggerDebugMonospace, None, |ui| {
        ui.add_sized(
            [240.0, 30.0],
            egui::Slider::new(&mut settings.ui.debug_monospace_scale, 0.75..=1.5).suffix("x"),
        )
    });

    let color_ids = [
        Id::DebuggerAddressColor,
        Id::DebuggerOpcodeBytesColor,
        Id::DebuggerMnemonicColor,
        Id::DebuggerSymbolColor,
        Id::DebuggerSourceColor,
        Id::DebuggerCurrentPcColor,
        Id::DebuggerChangedValueColor,
        Id::DebuggerBreakpointColor,
        Id::DebuggerWatchpointColor,
        Id::DebuggerSelectionColor,
        Id::DebuggerInterruptColor,
    ];
    let colors_requested = color_ids.into_iter().any(|id| search::requested(ui, id))
        || search::requested(ui, Id::DebuggerUseUiThemePalette)
        || search::requested(ui, Id::DebuggerResetDebuggerColors);
    egui::CollapsingHeader::new("Debugger colors")
        .id_salt("debugger_colors")
        .open(colors_requested.then_some(true))
        .show(ui, |ui| {
            egui::Grid::new("debugger_color_grid")
                .num_columns(3)
                .spacing([18.0, 8.0])
                .show(ui, |ui| {
                    color_row(
                        ui,
                        Id::DebuggerAddressColor,
                        "Address",
                        &mut settings.ui.debug_colors.address,
                    );
                    color_row(
                        ui,
                        Id::DebuggerOpcodeBytesColor,
                        "Opcode bytes",
                        &mut settings.ui.debug_colors.opcode,
                    );
                    color_row(
                        ui,
                        Id::DebuggerMnemonicColor,
                        "Mnemonic",
                        &mut settings.ui.debug_colors.mnemonic,
                    );
                    color_row(
                        ui,
                        Id::DebuggerSymbolColor,
                        "Symbol",
                        &mut settings.ui.debug_colors.symbol,
                    );
                    color_row(
                        ui,
                        Id::DebuggerSourceColor,
                        "Source",
                        &mut settings.ui.debug_colors.source,
                    );
                    color_row(
                        ui,
                        Id::DebuggerCurrentPcColor,
                        "Current PC",
                        &mut settings.ui.debug_colors.pc,
                    );
                    color_row(
                        ui,
                        Id::DebuggerChangedValueColor,
                        "Changed",
                        &mut settings.ui.debug_colors.changed,
                    );
                    color_row(
                        ui,
                        Id::DebuggerBreakpointColor,
                        "Breakpoint",
                        &mut settings.ui.debug_colors.breakpoint,
                    );
                    color_row(
                        ui,
                        Id::DebuggerWatchpointColor,
                        "Watchpoint",
                        &mut settings.ui.debug_colors.watchpoint,
                    );
                    color_row(
                        ui,
                        Id::DebuggerSelectionColor,
                        "Selection",
                        &mut settings.ui.debug_colors.selection,
                    );
                    color_row(
                        ui,
                        Id::DebuggerInterruptColor,
                        "Interrupt",
                        &mut settings.ui.debug_colors.interrupt,
                    );
                });
            ui.add_space(8.0);
            let palette_response = row(ui, Id::DebuggerUseUiThemePalette, Some("Palette"), |ui| {
                ui.button("Use UI theme palette")
            });
            search::target(ui, Id::DebuggerResetDebuggerColors, &palette_response);
            if palette_response.clicked() {
                settings.ui.debug_colors =
                    crate::settings::DebugColors::for_theme(settings.ui.theme_preset);
            }
        });

    ui.add_space(22.0);
    enum_combo(
        ui,
        Id::DebuggerDebuggerLayout,
        "debugger_layout",
        None,
        &mut settings.ui.debug_presentation,
    );
    checkbox(
        ui,
        Id::DebuggerShowFpsInDebugPanel,
        None,
        &mut settings.ui.show_fps,
    );
    checkbox(
        ui,
        Id::DebuggerEnableMemoryEditing,
        None,
        &mut settings.ui.enable_memory_editing,
    )
    .on_hover_text("Allow writing to memory addresses in the Memory Viewer");
}

fn color_row(ui: &mut egui::Ui, id: Id, label: &str, value: &mut [u8; 4]) {
    ui.label(label);
    let mut color = egui::Color32::from_rgba_unmultiplied(value[0], value[1], value[2], value[3]);
    let response = egui::color_picker::color_edit_button_srgba(
        ui,
        &mut color,
        egui::color_picker::Alpha::Opaque,
    );
    if response.changed() {
        *value = [color.r(), color.g(), color.b(), color.a()];
    }
    search::target(ui, id, &response);
    ui.monospace(format!(
        "#{:02X}{:02X}{:02X}",
        color.r(),
        color.g(),
        color.b()
    ));
    ui.end_row();
}
