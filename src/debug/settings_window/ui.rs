use crate::debug::ui_helpers::enum_combo_box;
use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label("Appearance, readability and window controls.");
    ui.add_space(8.0);

    let previous_theme = settings.ui.theme_preset;
    enum_combo_box(ui, "UI theme", &mut settings.ui.theme_preset);
    if previous_theme != settings.ui.theme_preset
        && settings.ui.debug_colors == crate::settings::DebugColors::for_theme(previous_theme)
    {
        settings.ui.debug_colors =
            crate::settings::DebugColors::for_theme(settings.ui.theme_preset);
    }
    if settings.ui.theme_preset != crate::settings::UiThemePreset::DefaultDark
        && settings.ui.debug_colors == crate::settings::DebugColors::default()
    {
        settings.ui.debug_colors =
            crate::settings::DebugColors::for_theme(settings.ui.theme_preset);
    }
    enum_combo_box(ui, "UI density", &mut settings.ui.ui_density);
    ui.checkbox(&mut settings.ui.autohide_menu_bar, "Autohide menu bar");

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
    });
}
