use super::MenuAction;
use crate::graphics::AspectRatioMode;
use crate::settings::Settings;

pub(super) fn draw(
    ui: &mut egui::Ui,
    actions: &mut Vec<MenuAction>,
    settings: &mut Settings,
    current_mode: AspectRatioMode,
) {
    if ui
        .selectable_label(current_mode == AspectRatioMode::Stretch, "Stretch")
        .clicked()
    {
        actions.push(MenuAction::SetAspectRatio(AspectRatioMode::Stretch));
        ui.close();
    }
    if ui
        .selectable_label(current_mode == AspectRatioMode::KeepAspect, "Keep Aspect")
        .clicked()
    {
        actions.push(MenuAction::SetAspectRatio(AspectRatioMode::KeepAspect));
        ui.close();
    }
    if ui
        .selectable_label(
            current_mode == AspectRatioMode::IntegerScale,
            "Integer Scale",
        )
        .clicked()
    {
        actions.push(MenuAction::SetAspectRatio(AspectRatioMode::IntegerScale));
        ui.close();
    }
    ui.separator();
    if ui.button("Fullscreen (F12)").clicked() {
        actions.push(MenuAction::ToggleFullscreen);
        ui.close();
    }
    ui.checkbox(&mut settings.ui.autohide_menu_bar, "Autohide menu bar")
        .on_hover_text("Hide the menu bar when the cursor is away from the top edge");
    ui.separator();
    ui.menu_button("Shader", |ui| {
        draw_shader_submenu(ui, actions, settings);
    });
}

fn draw_shader_submenu(ui: &mut egui::Ui, actions: &mut Vec<MenuAction>, settings: &mut Settings) {
    use crate::settings::{EffectPreset, ScalingMode};

    ui.label("Scaling");
    let scaling_modes = [
        (ScalingMode::PixelPerfect, "Pixel Perfect"),
        (ScalingMode::Bilinear, "Bilinear"),
        (ScalingMode::HQ2xLike, "HQ2x-like"),
        (ScalingMode::XBR2x, "xBR 2x"),
        (ScalingMode::Eagle2x, "Eagle 2x"),
    ];
    for (mode, label) in scaling_modes {
        if ui
            .selectable_label(settings.video.scaling_mode == mode, label)
            .clicked()
        {
            settings.video.scaling_mode = mode;
            actions.push(MenuAction::ToolbarSettingsChanged);
            ui.close();
        }
    }
    ui.separator();
    ui.label("Effect");
    let effects = [
        (EffectPreset::None, "None"),
        (EffectPreset::Scanlines, "Scanlines"),
        (EffectPreset::LcdGrid, "LCD Grid"),
        (EffectPreset::Crt, "CRT"),
        (EffectPreset::GbcPalette, "GBC Palette"),
        (EffectPreset::Custom, "Custom (file)"),
    ];
    for (effect, label) in effects {
        if ui
            .selectable_label(settings.video.effect_preset == effect, label)
            .clicked()
        {
            settings.video.effect_preset = effect;
            actions.push(MenuAction::ToolbarSettingsChanged);
            ui.close();
        }
    }
    if settings.video.effect_preset != EffectPreset::None
        || settings.video.scaling_mode.is_upscaler()
    {
        ui.separator();
        crate::debug::ui_helpers::draw_scaling_params(ui, settings);
        crate::debug::ui_helpers::draw_effect_params(ui, settings);
    }
}
