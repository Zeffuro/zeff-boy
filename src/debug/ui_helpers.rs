pub(crate) trait EnumLabel: Copy + PartialEq + 'static {
    fn label(self) -> &'static str;
    fn all_variants() -> &'static [Self];
}

pub(crate) fn enum_combo_box<E: EnumLabel>(ui: &mut egui::Ui, combo_label: &str, value: &mut E) {
    egui::ComboBox::from_label(combo_label)
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for &variant in E::all_variants() {
                ui.selectable_value(value, variant, variant.label());
            }
        });
}

pub(crate) fn draw_scaling_params(ui: &mut egui::Ui, settings: &mut crate::settings::Settings) {
    use crate::settings::ScalingMode;
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
}

pub(crate) fn draw_effect_params(ui: &mut egui::Ui, settings: &mut crate::settings::Settings) {
    use crate::settings::EffectPreset;
    let p = &mut settings.video.shader_params;
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
            if settings.video.custom_shader_path.is_empty() {
                ui.monospace("(not set)");
            } else {
                ui.monospace(&settings.video.custom_shader_path);
            }
            ui.horizontal(|ui| {
                if ui.button("Load .wgsl...").clicked()
                    && let Some(path) = crate::platform::FileDialog::new()
                        .add_filter("WGSL", &["wgsl"])
                        .pick_file()
                {
                    settings.video.custom_shader_path = path.to_string_lossy().to_string();
                }
                if ui.button("Clear").clicked() {
                    settings.video.custom_shader_path.clear();
                }
            });
        }
        EffectPreset::None => {}
    }
}
