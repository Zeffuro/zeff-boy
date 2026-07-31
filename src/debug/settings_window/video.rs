use crate::debug::ui_helpers::enum_combo_box;
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

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
        crate::debug::ui_helpers::draw_scaling_params(ui, settings);
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
    enum_combo_box(ui, "Effect", &mut settings.video.effect_preset);

    crate::debug::ui_helpers::draw_effect_params(ui, settings);

    ui.separator();
    ui.heading("Console Color");
    draw_gb_palette_section(
        ui,
        settings,
        active_system,
        gb_hardware_mode_label,
        is_pocket_camera,
    );

    ui.separator();
    draw_gba_display_section(ui, settings, active_system);

    ui.separator();
    draw_nes_palette_section(ui, settings, active_system);
}

fn draw_gb_palette_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
    gb_hardware_mode_label: Option<&str>,
    is_pocket_camera: bool,
) {
    use crate::settings::{ColorCorrection, DmgPalettePreset};

    super::draw_console_section_header(ui, "Game Boy", active_system, ActiveSystem::GameBoy);

    enum_combo_box(
        ui,
        "GB/GBC color correction",
        &mut settings.video.gb_color_correction,
    );
    if settings.video.gb_color_correction == ColorCorrection::Custom {
        draw_custom_color_matrix(
            ui,
            "gb_color_correction_matrix",
            &mut settings.video.gb_color_correction_matrix,
            true,
        );
    }
    ui.label(
        egui::RichText::new(
            "Applies as a display post-process to GB/GBC output. DMG palette selection below still controls the raw DMG colors before this correction.",
        )
        .weak()
        .small(),
    );

    let gb_mode = gb_hardware_mode_label.unwrap_or_default();
    let cgb_active = gb_mode.starts_with("CGB");
    let sgb_active = gb_mode.starts_with("SGB");
    let dmg_palette_applicable = !cgb_active && !sgb_active && !is_pocket_camera;

    ui.add_enabled_ui(dmg_palette_applicable, |ui| {
        enum_combo_box(ui, "DMG palette", &mut settings.video.gb_dmg_palette_preset);
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

    if settings.video.gb_dmg_palette_preset == DmgPalettePreset::DmgGreen {
        ui.label(
            egui::RichText::new("Classic pea-green DMG tone")
                .weak()
                .small(),
        );
    }
}

fn draw_gba_display_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
) {
    use crate::settings::GbaColorCorrection;

    super::draw_console_section_header(
        ui,
        "Game Boy Advance",
        active_system,
        ActiveSystem::GameBoyAdvance,
    );

    enum_combo_box(
        ui,
        "GBA color correction",
        &mut settings.video.gba_color_correction,
    );
    if settings.video.gba_color_correction == GbaColorCorrection::Custom {
        draw_custom_color_matrix(
            ui,
            "gba_color_correction_matrix",
            &mut settings.video.gba_color_correction_matrix,
            false,
        );
    }
    ui.label(
        egui::RichText::new(
            "Applies as a shader post-process to raw GBA RGB555 output. AGB LCD is a punchier handheld-screen simulation; LCD response is the stronger handheld LCD-response model.",
        )
        .weak()
        .small(),
    );
}

fn draw_nes_palette_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
) {
    use crate::settings::NesPaletteMode;

    super::draw_console_section_header(ui, "NES", active_system, ActiveSystem::Nes);

    enum_combo_box(ui, "NES palette mode", &mut settings.video.nes_palette_mode);
    if settings.video.nes_palette_mode == NesPaletteMode::Custom {
        ui.separator();
        ui.label("Custom NES .pal file:");
        ui.add(
            egui::TextEdit::singleline(&mut settings.video.nes_custom_palette_path)
                .hint_text("Path to 192-byte or 1536-byte binary .pal file")
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if ui.button("Load .pal...").clicked()
                && let Some(path) = crate::platform::FileDialog::new()
                    .add_filter("NES palette", &["pal"])
                    .pick_file()
            {
                settings.video.nes_custom_palette_path = path.to_string_lossy().to_string();
            }
            if ui.button("Clear").clicked() {
                settings.video.nes_custom_palette_path.clear();
            }
        });

        match nes_palette_path_status(&settings.video.nes_custom_palette_path) {
            Ok(message) => {
                ui.label(egui::RichText::new(message).weak().small());
            }
            Err(message) => {
                ui.label(
                    egui::RichText::new(message)
                        .color(egui::Color32::RED)
                        .small(),
                );
            }
        }
    } else if settings.video.nes_palette_mode != NesPaletteMode::Raw {
        ui.label(
            egui::RichText::new("Applies to NES rendering and NES palette debug views.")
                .weak()
                .small(),
        );
    }
}

fn nes_palette_path_status(path: &str) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(
            "No custom NES palette file selected; rendering will fall back to raw.".to_string(),
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let bytes =
            std::fs::read(path).map_err(|err| format!("Could not read .pal file: {err}"))?;
        zeff_nes_core::hardware::ppu::parse_nes_palette_bytes(&bytes)
            .map_err(|err| format!("Invalid .pal file: {err}"))?;
        Ok("Valid binary .pal file. 192-byte files provide the base palette; 1536-byte files also provide emphasis groups.".to_string())
    }

    #[cfg(target_arch = "wasm32")]
    {
        Err("Custom NES palette files are not available in browser builds yet.".to_string())
    }
}

fn draw_custom_color_matrix(
    ui: &mut egui::Ui,
    grid_id: &'static str,
    matrix: &mut [f32; 9],
    include_gbc_preset: bool,
) {
    ui.separator();
    ui.label("Custom 3x3 matrix (input RGB -> output RGB)");

    egui::Grid::new(grid_id).spacing([6.0, 4.0]).show(ui, |ui| {
        ui.label("R'");
        ui.add(
            egui::DragValue::new(&mut matrix[0])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.add(
            egui::DragValue::new(&mut matrix[1])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.add(
            egui::DragValue::new(&mut matrix[2])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.end_row();

        ui.label("G'");
        ui.add(
            egui::DragValue::new(&mut matrix[3])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.add(
            egui::DragValue::new(&mut matrix[4])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.add(
            egui::DragValue::new(&mut matrix[5])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.end_row();

        ui.label("B'");
        ui.add(
            egui::DragValue::new(&mut matrix[6])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.add(
            egui::DragValue::new(&mut matrix[7])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.add(
            egui::DragValue::new(&mut matrix[8])
                .speed(0.01)
                .range(-2.0..=2.0),
        );
        ui.end_row();
    });

    ui.horizontal(|ui| {
        if ui.button("Identity").clicked() {
            *matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        }
        if include_gbc_preset && ui.button("Load GBC matrix").clicked() {
            *matrix = [
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
