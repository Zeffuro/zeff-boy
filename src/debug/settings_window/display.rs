use crate::settings::Settings;

pub(super) fn draw(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Display");
    ui.checkbox(&mut settings.show_fps, "Show FPS in debug panel");
    ui.checkbox(&mut settings.enable_memory_editing, "Enable memory editing")
        .on_hover_text("Allow writing to memory addresses in the Memory Viewer");
    ui.checkbox(&mut settings.autohide_menu_bar, "Autohide menu bar")
        .on_hover_text(
            "Hide the menu bar when the cursor moves away from the top edge. \
             Hover near the top to reveal it.",
        );

    use crate::settings::VsyncMode;
    egui::ComboBox::from_label("VSync")
        .selected_text(settings.vsync_mode.label())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut settings.vsync_mode, VsyncMode::On, VsyncMode::On.label());
            ui.selectable_value(
                &mut settings.vsync_mode,
                VsyncMode::Adaptive,
                VsyncMode::Adaptive.label(),
            );
            ui.selectable_value(
                &mut settings.vsync_mode,
                VsyncMode::Off,
                VsyncMode::Off.label(),
            );
        });

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
            .find(|(v, _)| (*v - settings.ui_scale).abs() < 0.01)
            .map(|(_, l)| *l)
            .unwrap_or("Custom");
        egui::ComboBox::from_label("UI scale")
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                for &(value, label) in SCALES {
                    ui.selectable_value(&mut settings.ui_scale, value, label);
                }
            });
    })
    .response
    .on_hover_text("Scale all UI elements (menu bar, debug panels, toasts).");

    ui.separator();
    ui.heading("Scaling");
    use crate::settings::ScalingMode;
    egui::ComboBox::from_label("Scaling mode")
        .selected_text(settings.scaling_mode.label())
        .show_ui(ui, |ui| {
            for mode in [
                ScalingMode::PixelPerfect,
                ScalingMode::Bilinear,
                ScalingMode::HQ2xLike,
                ScalingMode::XBR2x,
                ScalingMode::Eagle2x,
            ] {
                ui.selectable_value(&mut settings.scaling_mode, mode, mode.label());
            }
        });

    if settings.scaling_mode.is_upscaler() {
        let p = &mut settings.shader_params;
        match settings.scaling_mode {
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
            egui::DragValue::new(&mut settings.offscreen_scale)
                .range(1..=8)
                .speed(1),
        );
        ui.label(format!(
            "({}×{})",
            160 * settings.offscreen_scale,
            144 * settings.offscreen_scale
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
    egui::ComboBox::from_label("Effect")
        .selected_text(settings.effect_preset.label())
        .show_ui(ui, |ui| {
            for effect in [
                EffectPreset::None,
                EffectPreset::Scanlines,
                EffectPreset::LCDGrid,
                EffectPreset::CRT,
                EffectPreset::GbcPalette,
                EffectPreset::Custom,
            ] {
                ui.selectable_value(&mut settings.effect_preset, effect, effect.label());
            }
        });

    let p = &mut settings.shader_params;
    match settings.effect_preset {
        EffectPreset::Scanlines => {
            ui.add(egui::Slider::new(&mut p.scanline_intensity, 0.0..=1.0).text("Intensity"));
        }
        EffectPreset::LCDGrid => {
            ui.add(egui::Slider::new(&mut p.grid_intensity, 0.0..=1.0).text("Grid"));
        }
        EffectPreset::CRT => {
            ui.add(egui::Slider::new(&mut p.scanline_intensity, 0.0..=1.0).text("Scanlines"));
            ui.add(egui::Slider::new(&mut p.crt_curvature, 0.0..=1.0).text("Curvature"));
        }
        EffectPreset::GbcPalette => {
            ui.add(egui::Slider::new(&mut p.palette_mix, 0.0..=1.0).text("Palette Mix"));
            ui.add(egui::Slider::new(&mut p.palette_warmth, 0.0..=1.0).text("Warmth"));
        }
        EffectPreset::Custom => {
            ui.label("Custom WGSL fragment path:");
            if settings.custom_shader_path.is_empty() {
                ui.monospace("(not set)");
            } else {
                ui.monospace(&settings.custom_shader_path);
            }
            ui.horizontal(|ui| {
                if ui.button("Load .wgsl...").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("WGSL", &["wgsl"])
                        .pick_file()
                {
                    settings.custom_shader_path = path.to_string_lossy().to_string();
                }
                if ui.button("Clear").clicked() {
                    settings.custom_shader_path.clear();
                }
            });
        }
        EffectPreset::None => {}
    }

    ui.separator();
    ui.heading("Color Correction");
    use crate::settings::ColorCorrection;
    egui::ComboBox::from_label("Color correction")
        .selected_text(settings.color_correction.label())
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut settings.color_correction,
                ColorCorrection::None,
                ColorCorrection::None.label(),
            );
            ui.selectable_value(
                &mut settings.color_correction,
                ColorCorrection::GbcLcd,
                ColorCorrection::GbcLcd.label(),
            );
            ui.selectable_value(
                &mut settings.color_correction,
                ColorCorrection::Custom,
                ColorCorrection::Custom.label(),
            );
        });
    if settings.color_correction == ColorCorrection::Custom {
        ui.separator();
        ui.label("Custom 3x3 matrix (input RGB -> output RGB)");

        let m = &mut settings.color_correction_matrix;
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
                settings.color_correction_matrix = [
                    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
                ];
            }
            if ui.button("Load GBC matrix").clicked() {
                settings.color_correction_matrix = [
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

