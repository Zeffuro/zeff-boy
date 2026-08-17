use crate::debug::ui_helpers::enum_combo_box;
use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("UI");

    enum_combo_box(ui, "UI theme", &mut settings.ui.theme_preset);
    enum_combo_box(ui, "UI density", &mut settings.ui.ui_density);
    ui.add(
        egui::Slider::new(&mut settings.ui.debug_monospace_scale, 0.75..=1.5)
            .text("Debug monospace")
            .suffix("x"),
    );
    ui.collapsing("Debugger colors", |ui| {
        color_row(ui, "Address", &mut settings.ui.debug_colors.address);
        color_row(ui, "Symbol", &mut settings.ui.debug_colors.symbol);
        color_row(ui, "Current PC", &mut settings.ui.debug_colors.pc);
        color_row(ui, "Changed", &mut settings.ui.debug_colors.changed);
        color_row(ui, "Breakpoint", &mut settings.ui.debug_colors.breakpoint);
        color_row(ui, "Watchpoint", &mut settings.ui.debug_colors.watchpoint);
        if ui.small_button("Reset debugger colors").clicked() {
            settings.ui.debug_colors = crate::settings::DebugColors::default();
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
    ui.checkbox(&mut settings.ui.autohide_menu_bar, "Autohide menu bar")
        .on_hover_text(
            "Hide the menu bar when the cursor moves away from the top edge. \
             Hover near the top to reveal it.",
        );

    ui.horizontal(|ui| {
        const SCALES: &[(f32, &str)] = &[
            (0.75, "75%"),
            (1.0, "100%"),
            (1.25, "125%"),
            (1.5, "150%"),
            (1.75, "175%"),
            (2.0, "200%"),
            (2.5, "250%"),
            (3.0, "300%"),
        ];
        let current_label = SCALES
            .iter()
            .find(|(v, _)| (*v - settings.ui.ui_scale).abs() < 0.01)
            .map(|(_, l)| *l)
            .unwrap_or("Custom");
        egui::ComboBox::from_label("UI scale")
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                for &(value, label) in SCALES {
                    ui.selectable_value(&mut settings.ui.ui_scale, value, label);
                }
            });
    })
    .response
    .on_hover_text("Scale all UI elements (menu bar, debug panels, toasts).");
}

fn color_row(ui: &mut egui::Ui, label: &str, value: &mut [u8; 4]) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut color =
            egui::Color32::from_rgba_unmultiplied(value[0], value[1], value[2], value[3]);
        if egui::color_picker::color_edit_button_srgba(
            ui,
            &mut color,
            egui::color_picker::Alpha::Opaque,
        )
        .changed()
        {
            *value = [color.r(), color.g(), color.b(), color.a()];
        }
    });
}
