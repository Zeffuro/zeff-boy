use crate::settings::Settings;

use super::{
    layout::{checkbox, enum_combo, helper, row},
    search::SettingId as Id,
};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    helper(ui, "Appearance, readability and window controls.");
    ui.add_space(14.0);

    let previous_theme = settings.ui.theme_preset;
    enum_combo(
        ui,
        Id::InterfaceUiTheme,
        "interface_theme",
        None,
        &mut settings.ui.theme_preset,
    );
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
    enum_combo(
        ui,
        Id::InterfaceUiDensity,
        "interface_density",
        None,
        &mut settings.ui.ui_density,
    );
    checkbox(
        ui,
        Id::InterfaceAutohideMenuBar,
        None,
        &mut settings.ui.autohide_menu_bar,
    );

    row(ui, Id::InterfaceUiScale, None, |ui| {
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
            .find(|(value, _)| (*value - settings.ui.ui_scale).abs() < 0.01)
            .map(|(_, label)| *label)
            .unwrap_or("Custom");
        egui::ComboBox::from_id_salt("interface_scale")
            .selected_text(current_label)
            .width(220.0)
            .show_ui(ui, |ui| {
                for &(value, label) in SCALES {
                    ui.selectable_value(&mut settings.ui.ui_scale, value, label);
                }
            })
            .response
    });
}
