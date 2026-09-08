use crate::debug::ui_helpers::enum_combo_box;
use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label("Appearance and editing tools for inspecting a running game.");
    ui.add_space(8.0);
    ui.add(
        egui::Slider::new(&mut settings.ui.debug_monospace_scale, 0.75..=1.5)
            .text("Debug monospace")
            .suffix("x"),
    );
    egui::CollapsingHeader::new("Debugger colors")
        .id_salt("debugger_colors")
        .show(ui, |ui| {
            egui::Grid::new("debugger_color_grid")
                .num_columns(3)
                .spacing([18.0, 8.0])
                .show(ui, |ui| {
                    color_row(ui, "Address", &mut settings.ui.debug_colors.address);
                    color_row(ui, "Opcode bytes", &mut settings.ui.debug_colors.opcode);
                    color_row(ui, "Mnemonic", &mut settings.ui.debug_colors.mnemonic);
                    color_row(ui, "Symbol", &mut settings.ui.debug_colors.symbol);
                    color_row(ui, "Source", &mut settings.ui.debug_colors.source);
                    color_row(ui, "Current PC", &mut settings.ui.debug_colors.pc);
                    color_row(ui, "Changed", &mut settings.ui.debug_colors.changed);
                    color_row(ui, "Breakpoint", &mut settings.ui.debug_colors.breakpoint);
                    color_row(ui, "Watchpoint", &mut settings.ui.debug_colors.watchpoint);
                    color_row(ui, "Selection", &mut settings.ui.debug_colors.selection);
                    color_row(ui, "Interrupt", &mut settings.ui.debug_colors.interrupt);
                });
            ui.add_space(8.0);
            if ui.button("Use UI theme palette").clicked() {
                settings.ui.debug_colors =
                    crate::settings::DebugColors::for_theme(settings.ui.theme_preset);
            }
        });
    enum_combo_box(ui, "Debugger layout", &mut settings.ui.debug_presentation);
    ui.add_space(4.0);

    ui.checkbox(&mut settings.ui.show_fps, "Show FPS in debug panel");
    ui.checkbox(
        &mut settings.ui.enable_memory_editing,
        "Enable memory editing",
    )
    .on_hover_text("Allow writing to memory addresses in the Memory Viewer");
}

fn color_row(ui: &mut egui::Ui, label: &str, value: &mut [u8; 4]) {
    ui.label(label);
    let mut color = egui::Color32::from_rgba_unmultiplied(value[0], value[1], value[2], value[3]);
    if egui::color_picker::color_edit_button_srgba(
        ui,
        &mut color,
        egui::color_picker::Alpha::Opaque,
    )
    .changed()
    {
        *value = [color.r(), color.g(), color.b(), color.a()];
    }
    ui.monospace(format!(
        "#{:02X}{:02X}{:02X}",
        color.r(),
        color.g(),
        color.b()
    ));
    ui.end_row();
}
