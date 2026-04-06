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
        let p = &mut settings.video.shader_params;
        match settings.video.scaling_mode {
            ScalingMode::HQ2xLike => {
                ui.add(
                    egui::Slider::new(&mut p.upscale_edge_strength, 0.0..=2.0)
                        .text("Edge Strength"),
                );
            }
            ScalingMode::XBR2x => {
                ui.add(
                    egui::Slider::new(&mut p.upscale_edge_strength, 0.1..=2.0)
                        .text("Edge Strength"),
                );
            }
            ScalingMode::Eagle2x => {
                ui.add(
                    egui::Slider::new(&mut p.upscale_edge_strength, 0.0..=1.0)
                        .text("Edge Strength"),
                );
            }
            _ => {}
        }
        match settings.video.effect_preset {
            EffectPreset::Scanlines => {
                ui.add(egui::Slider::new(&mut p.scanline_intensity, 0.0..=1.0).text("Intensity"));
            }
            EffectPreset::LcdGrid => {
                ui.add(egui::Slider::new(&mut p.grid_intensity, 0.0..=1.0).text("Grid"));
            }
            EffectPreset::Crt => {
                ui.add(egui::Slider::new(&mut p.scanline_intensity, 0.0..=1.0).text("Scanlines"));
                ui.add(egui::Slider::new(&mut p.crt_curvature, 0.0..=1.0).text("Curvature"));
            }
            EffectPreset::GbcPalette => {
                ui.add(egui::Slider::new(&mut p.palette_mix, 0.0..=1.0).text("Palette Mix"));
                ui.add(egui::Slider::new(&mut p.palette_warmth, 0.0..=1.0).text("Warmth"));
            }
            EffectPreset::Custom => {
                ui.label("Custom WGSL fragment path:");
                ui.monospace(if settings.video.custom_shader_path.is_empty() {
                    "(not set)".to_string()
                } else {
                    settings.video.custom_shader_path.clone()
                });
                if ui.button("Load .wgsl...").clicked() {
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("WGSL", &["wgsl"])
                        .pick_file()
                    {
                        settings.video.custom_shader_path = path.to_string_lossy().to_string();
                        actions.push(MenuAction::ToolbarSettingsChanged);
                    }
                }
                if ui.button("Clear custom shader").clicked() {
                    settings.video.custom_shader_path.clear();
                    actions.push(MenuAction::ToolbarSettingsChanged);
                }
            }
            EffectPreset::None => {}
        }
    }
}
