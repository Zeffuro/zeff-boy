use crate::debug::ui_helpers::enum_combo_box;
use crate::emu_backend::ActiveSystem;
use crate::settings::{ScalingMode, Settings};

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
    gb_hardware_mode_label: Option<&str>,
    is_pocket_camera: bool,
) {
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
        draw_custom_color_matrix(ui, settings);
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

    ui.separator();
    draw_gb_palette_section(
        ui,
        settings,
        active_system,
        gb_hardware_mode_label,
        is_pocket_camera,
    );

    ui.separator();
    draw_nes_palette_section(ui, settings, active_system);
}

fn draw_console_section_header(
    ui: &mut egui::Ui,
    label: &str,
    active_system: Option<ActiveSystem>,
    target: ActiveSystem,
) {
    ui.horizontal(|ui| {
        ui.heading(label);
        if active_system == Some(target) {
            ui.label(egui::RichText::new("(active)").weak().italics().small());
        }
    });
}

fn draw_gb_palette_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
    gb_hardware_mode_label: Option<&str>,
    is_pocket_camera: bool,
) {
    use crate::settings::DmgPalettePreset;

    draw_console_section_header(ui, "Game Boy", active_system, ActiveSystem::GameBoy);

    let gb_mode = gb_hardware_mode_label.unwrap_or_default();
    let cgb_active = gb_mode.starts_with("CGB");
    let sgb_active = gb_mode.starts_with("SGB");
    let dmg_palette_applicable = !cgb_active && !sgb_active && !is_pocket_camera;

    ui.add_enabled_ui(dmg_palette_applicable, |ui| {
        enum_combo_box(ui, "DMG palette", &mut settings.video.dmg_palette_preset);
    });

    if !gb_mode.is_empty() {
        if cgb_active {
            ui.label(
                egui::RichText::new(
                    "Current game is running in CGB mode. DMG palettes apply to DMG rendering only.",
                )
                .weak()
                .small(),
            );
        } else if sgb_active {
            ui.label(
                egui::RichText::new(
                    "Current game is running in SGB mode. SGB palettes/borders override DMG palette presets.",
                )
                .weak()
                .small(),
            );
        } else if is_pocket_camera {
            ui.label(
                egui::RichText::new(
                    "Pocket Camera output uses cartridge-specific grayscale behavior; DMG palette presets are not applied.",
                )
                .weak()
                .small(),
            );
        } else {
            ui.label(
                egui::RichText::new("DMG palette preset is active for the current game.")
                    .weak()
                    .small(),
            );
        }
    }

    if settings.video.dmg_palette_preset == DmgPalettePreset::DmgGreen {
        ui.label(
            egui::RichText::new("Classic pea-green DMG tone")
                .weak()
                .small(),
        );
    }
}

fn draw_nes_palette_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
) {
    use crate::settings::NesPaletteMode;

    draw_console_section_header(ui, "NES", active_system, ActiveSystem::Nes);

    enum_combo_box(ui, "NES palette mode", &mut settings.video.nes_palette_mode);
    if settings.video.nes_palette_mode != NesPaletteMode::Raw {
        ui.label(
            egui::RichText::new("Applies to NES rendering and NES palette debug views.")
                .weak()
                .small(),
        );
    }
}

fn draw_custom_color_matrix(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.separator();
    ui.label("Custom 3x3 matrix (input RGB -> output RGB)");

    let m = &mut settings.video.color_correction_matrix;
    egui::Grid::new("color_correction_matrix")
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            ui.label("R'");
            ui.add(
                egui::DragValue::new(&mut m[0])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.add(
                egui::DragValue::new(&mut m[1])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.add(
                egui::DragValue::new(&mut m[2])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.end_row();

            ui.label("G'");
            ui.add(
                egui::DragValue::new(&mut m[3])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.add(
                egui::DragValue::new(&mut m[4])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.add(
                egui::DragValue::new(&mut m[5])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.end_row();

            ui.label("B'");
            ui.add(
                egui::DragValue::new(&mut m[6])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.add(
                egui::DragValue::new(&mut m[7])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.add(
                egui::DragValue::new(&mut m[8])
                    .speed(0.01)
                    .range(-2.0..=2.0),
            );
            ui.end_row();
        });

    ui.horizontal(|ui| {
        if ui.button("Identity").clicked() {
            settings.video.color_correction_matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
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
