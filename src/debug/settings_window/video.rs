use crate::debug::ui_helpers::enum_combo_box;
use crate::settings::{ScalingMode, Settings};

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Video");
    enum_combo_box(ui, "VSync", &mut settings.video.vsync_mode);

    ui.separator();
    ui.heading("Scaling");
    enum_combo_box(ui, "Scaling mode", &mut settings.video.scaling_mode);

    if settings.video.scaling_mode.is_upscaler() {
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

    ui.horizontal(|ui| {
        ui.label("Offscreen scale:");
        ui.add(
            egui::DragValue::new(&mut settings.video.offscreen_scale)
                .range(1..=8)
                .speed(1),
        );
        ui.label(format!(
            "({}x{})",
            160 * settings.video.offscreen_scale,
            144 * settings.video.offscreen_scale
        ));
    });
    ui.label(
        egui::RichText::new(
            "Applies to the Game View dock tab only. Direct rendering uses the window resolution.",
        )
        .small()
        .weak(),
    );

    ui.separator();
    ui.heading("Effects");
    use crate::settings::EffectPreset;
    enum_combo_box(ui, "Effect", &mut settings.video.effect_preset);

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
                    && let Some(path) = rfd::FileDialog::new()
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

    ui.separator();
    ui.heading("Color Correction");
    use crate::settings::ColorCorrection;
    enum_combo_box(ui, "Color correction", &mut settings.video.color_correction);
    if settings.video.color_correction == ColorCorrection::Custom {
        ui.separator();
        ui.label("Custom 3x3 matrix (input RGB -> output RGB)");

        let m = &mut settings.video.color_correction_matrix;
        egui::Grid::new("color_correction_matrix")
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("R'");
                ui.add(egui::DragValue::new(&mut m[0]).speed(0.01).range(-2.0..=2.0));
                ui.add(egui::DragValue::new(&mut m[1]).speed(0.01).range(-2.0..=2.0));
                ui.add(egui::DragValue::new(&mut m[2]).speed(0.01).range(-2.0..=2.0));
                ui.end_row();

                ui.label("G'");
                ui.add(egui::DragValue::new(&mut m[3]).speed(0.01).range(-2.0..=2.0));
                ui.add(egui::DragValue::new(&mut m[4]).speed(0.01).range(-2.0..=2.0));
                ui.add(egui::DragValue::new(&mut m[5]).speed(0.01).range(-2.0..=2.0));
                ui.end_row();

                ui.label("B'");
                ui.add(egui::DragValue::new(&mut m[6]).speed(0.01).range(-2.0..=2.0));
                ui.add(egui::DragValue::new(&mut m[7]).speed(0.01).range(-2.0..=2.0));
                ui.add(egui::DragValue::new(&mut m[8]).speed(0.01).range(-2.0..=2.0));
                ui.end_row();
            });

        ui.horizontal(|ui| {
            if ui.button("Identity").clicked() {
                settings.video.color_correction_matrix = [
                    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
                ];
            }
            if ui.button("Load GBC matrix").clicked() {
                settings.video.color_correction_matrix = [
                    26.0 / 32.0,
                    4.0 / 32.0,
                    2.0 / 32.0,
                    0.0,
                    24.0 / 32.0,
                    8.0 / 32.0,
                    6.0 / 32.0,
                    4.0 / 32.0,
                    22.0 / 32.0,
                ];
            }
        });
    }
    ui.label(
        egui::RichText::new(
            "None: raw RGB555 colors expanded to 8-bit per channel.\n\
             GBC LCD: simulates the color response of the Game Boy Color LCD panel,\n\
             which shifts colors toward a warmer, slightly washed-out appearance.\n\
             Custom matrix: apply your own 3x3 RGB transform.",
        )
        .weak()
        .small(),
    );
}

