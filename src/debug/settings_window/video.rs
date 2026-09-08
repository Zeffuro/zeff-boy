use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

use super::{
    layout::{enum_combo, helper, row},
    search::{self, SettingId as Id},
};

pub(super) fn draw(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
    gb_hardware_mode_label: Option<&str>,
    is_pocket_camera: bool,
    #[cfg(target_arch = "wasm32")] nes_palette_file_slot: crate::platform::FileDataSlot,
) {
    helper(
        ui,
        egui::RichText::new("Presentation, scaling, and console color output.")
            .small()
            .weak(),
    );
    ui.add_space(14.0);
    enum_combo(
        ui,
        Id::VideoVsync,
        "video_vsync",
        None,
        &mut settings.video.vsync_mode,
    );

    ui.add_space(22.0);
    ui.heading("Scaling");
    enum_combo(
        ui,
        Id::VideoScalingMode,
        "video_scaling_mode",
        None,
        &mut settings.video.scaling_mode,
    );

    search::conditional(
        ui,
        Id::VideoEdgeStrength,
        Id::VideoScalingMode,
        settings.video.scaling_mode.is_upscaler(),
        "Choose an upscaling mode to edit edge strength.",
    );
    if settings.video.scaling_mode.is_upscaler() {
        draw_scaling_params(ui, settings);
    }

    row(ui, Id::VideoOffscreenScale, None, |ui| {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::DragValue::new(&mut settings.video.offscreen_scale)
                    .range(1..=8)
                    .speed(1),
            );
            ui.label(format!(
                "({}x{})",
                160 * settings.video.offscreen_scale,
                144 * settings.video.offscreen_scale
            ));
            response
        })
        .inner
    });

    ui.add_space(22.0);
    ui.heading("Effects");
    enum_combo(
        ui,
        Id::VideoEffect,
        "video_effect",
        None,
        &mut settings.video.effect_preset,
    );

    draw_effect_params(ui, settings);

    ui.add_space(22.0);
    ui.heading("Console color");
    draw_gb_palette_section(
        ui,
        settings,
        active_system,
        gb_hardware_mode_label,
        is_pocket_camera,
    );

    ui.add_space(22.0);
    draw_gba_display_section(ui, settings, active_system);

    ui.add_space(22.0);
    draw_wonderswan_display_section(ui, settings, active_system);

    ui.add_space(22.0);
    draw_nes_palette_section(
        ui,
        settings,
        active_system,
        #[cfg(target_arch = "wasm32")]
        nes_palette_file_slot,
    );

    ui.add_space(22.0);
    draw_pce_display_section(ui, settings, active_system);
}

fn draw_scaling_params(ui: &mut egui::Ui, settings: &mut Settings) {
    use crate::settings::ScalingMode;

    let params = &mut settings.video.shader_params;
    let range = match settings.video.scaling_mode {
        ScalingMode::HQ2xLike => Some(0.0..=2.0),
        ScalingMode::XBR2x => Some(0.1..=2.0),
        ScalingMode::Eagle2x => Some(0.0..=1.0),
        _ => None,
    };
    if let Some(range) = range {
        row(ui, Id::VideoEdgeStrength, None, |ui| {
            ui.add_sized(
                [240.0, 30.0],
                egui::Slider::new(&mut params.upscale_edge_strength, range),
            )
        });
    }
}

fn draw_effect_params(ui: &mut egui::Ui, settings: &mut Settings) {
    use crate::settings::EffectPreset;

    let effect = settings.video.effect_preset;
    let params = &mut settings.video.shader_params;
    for (id, available, explanation) in [
        (
            Id::VideoScanlineIntensity,
            matches!(effect, EffectPreset::Scanlines | EffectPreset::Crt),
            "Choose Scanlines or CRT to edit scanline intensity.",
        ),
        (
            Id::VideoLcdGridIntensity,
            effect == EffectPreset::LcdGrid,
            "Choose LCD Grid to edit grid intensity.",
        ),
        (
            Id::VideoCrtCurvature,
            effect == EffectPreset::Crt,
            "Choose CRT to edit curvature.",
        ),
        (
            Id::VideoPaletteMix,
            effect == EffectPreset::GbcPalette,
            "Choose GBC Palette to edit palette mix.",
        ),
        (
            Id::VideoPaletteWarmth,
            effect == EffectPreset::GbcPalette,
            "Choose GBC Palette to edit palette warmth.",
        ),
        (
            Id::VideoLoadCustomWgslShader,
            effect == EffectPreset::Custom,
            "Choose Custom to load a WGSL shader.",
        ),
        (
            Id::VideoClearCustomShader,
            effect == EffectPreset::Custom,
            "Choose Custom to clear its WGSL shader.",
        ),
    ] {
        search::conditional(ui, id, Id::VideoEffect, available, explanation);
    }
    match effect {
        EffectPreset::Scanlines => {
            row(ui, Id::VideoScanlineIntensity, None, |ui| {
                ui.add_sized(
                    [240.0, 30.0],
                    egui::Slider::new(&mut params.scanline_intensity, 0.0..=1.0),
                )
            });
        }
        EffectPreset::LcdGrid => {
            row(ui, Id::VideoLcdGridIntensity, None, |ui| {
                ui.add_sized(
                    [240.0, 30.0],
                    egui::Slider::new(&mut params.grid_intensity, 0.0..=1.0),
                )
            });
        }
        EffectPreset::Crt => {
            row(ui, Id::VideoScanlineIntensity, None, |ui| {
                ui.add_sized(
                    [240.0, 30.0],
                    egui::Slider::new(&mut params.scanline_intensity, 0.0..=1.0),
                )
            });
            row(ui, Id::VideoCrtCurvature, None, |ui| {
                ui.add_sized(
                    [240.0, 30.0],
                    egui::Slider::new(&mut params.crt_curvature, 0.0..=1.0),
                )
            });
        }
        EffectPreset::GbcPalette => {
            row(ui, Id::VideoPaletteMix, None, |ui| {
                ui.add_sized(
                    [240.0, 30.0],
                    egui::Slider::new(&mut params.palette_mix, 0.0..=1.0),
                )
            });
            row(ui, Id::VideoPaletteWarmth, None, |ui| {
                ui.add_sized(
                    [240.0, 30.0],
                    egui::Slider::new(&mut params.palette_warmth, 0.0..=1.0),
                )
            });
        }
        EffectPreset::Custom => {
            row(
                ui,
                Id::VideoLoadCustomWgslShader,
                Some("Custom WGSL fragment path"),
                |ui| {
                    ui.monospace(if settings.video.custom_shader_path.is_empty() {
                        "(not set)"
                    } else {
                        &settings.video.custom_shader_path
                    })
                },
            );
            if row(ui, Id::VideoLoadCustomWgslShader, None, |ui| {
                ui.button("Load .wgsl...")
            })
            .clicked()
                && let Some(path) = crate::platform::FileDialog::new()
                    .add_filter("WGSL", &["wgsl"])
                    .pick_file()
            {
                settings.video.custom_shader_path = path.to_string_lossy().to_string();
            }
            if row(ui, Id::VideoClearCustomShader, None, |ui| {
                ui.button("Clear")
            })
            .clicked()
            {
                settings.video.custom_shader_path.clear();
            }
        }
        EffectPreset::None => {}
    }
}

fn draw_pce_display_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
) {
    super::draw_console_section_header(ui, "PC Engine", active_system, ActiveSystem::Pce);
    enum_combo(
        ui,
        Id::VideoPcEngineVisibleArea,
        "video_pce_visible_area",
        None,
        &mut settings.video.pce_overscan_mode,
    );
    enum_combo(
        ui,
        Id::VideoPcEngineColorOutput,
        "video_pce_color_output",
        None,
        &mut settings.video.pce_palette_mode,
    );
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

    enum_combo(
        ui,
        Id::VideoGbGbcColorCorrection,
        "video_gb_color_correction",
        None,
        &mut settings.video.gb_color_correction,
    );
    search::conditional(
        ui,
        Id::VideoGbGbcCustomColorMatrix,
        Id::VideoGbGbcColorCorrection,
        settings.video.gb_color_correction == ColorCorrection::Custom,
        "Choose Custom color correction to edit the GB/GBC matrix.",
    );
    if settings.video.gb_color_correction == ColorCorrection::Custom {
        draw_custom_color_matrix(
            ui,
            Id::VideoGbGbcCustomColorMatrix,
            "gb_color_correction_matrix",
            &mut settings.video.gb_color_correction_matrix,
            Some("Load GBC matrix"),
        );
    }

    let gb_mode = gb_hardware_mode_label.unwrap_or_default();
    let cgb_active = gb_mode.starts_with("CGB");
    let sgb_active = gb_mode.starts_with("SGB");
    let dmg_palette_applicable = !cgb_active && !sgb_active && !is_pocket_camera;

    ui.add_enabled_ui(dmg_palette_applicable, |ui| {
        enum_combo(
            ui,
            Id::VideoDmgPalette,
            "video_dmg_palette",
            None,
            &mut settings.video.gb_dmg_palette_preset,
        );
    });

    if !gb_mode.is_empty() {
        if cgb_active {
            ui.label(
                egui::RichText::new("DMG palette inactive in CGB mode.")
                    .weak()
                    .small(),
            );
        } else if sgb_active {
            ui.label(
                egui::RichText::new("SGB palette overrides DMG palette.")
                    .weak()
                    .small(),
            );
        } else if is_pocket_camera {
            ui.label(
                egui::RichText::new("Pocket Camera uses its own grayscale.")
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

    enum_combo(
        ui,
        Id::VideoGbaColorCorrection,
        "video_gba_color_correction",
        None,
        &mut settings.video.gba_color_correction,
    );
    search::conditional(
        ui,
        Id::VideoGbaCustomColorMatrix,
        Id::VideoGbaColorCorrection,
        settings.video.gba_color_correction == GbaColorCorrection::Custom,
        "Choose Custom color correction to edit the GBA matrix.",
    );
    if settings.video.gba_color_correction == GbaColorCorrection::Custom {
        draw_custom_color_matrix(
            ui,
            Id::VideoGbaCustomColorMatrix,
            "gba_color_correction_matrix",
            &mut settings.video.gba_color_correction_matrix,
            None,
        );
    }
}

fn draw_wonderswan_display_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
) {
    use crate::settings::WonderSwanColorCorrection;

    super::draw_console_section_header(ui, "WonderSwan", active_system, ActiveSystem::WonderSwan);

    enum_combo(
        ui,
        Id::VideoWsColorCorrection,
        "video_ws_color_correction",
        None,
        &mut settings.video.ws_color_correction,
    );
    search::conditional(
        ui,
        Id::VideoWonderswanCustomColorMatrix,
        Id::VideoWsColorCorrection,
        settings.video.ws_color_correction == WonderSwanColorCorrection::Custom,
        "Choose Custom color correction to edit the WonderSwan matrix.",
    );
    if settings.video.ws_color_correction == WonderSwanColorCorrection::Custom {
        draw_custom_color_matrix(
            ui,
            Id::VideoWonderswanCustomColorMatrix,
            "ws_color_correction_matrix",
            &mut settings.video.ws_color_correction_matrix,
            Some("Load WSC LCD matrix"),
        );
    }
}

fn draw_nes_palette_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    active_system: Option<ActiveSystem>,
    #[cfg(target_arch = "wasm32")] nes_palette_file_slot: crate::platform::FileDataSlot,
) {
    use crate::settings::NesPaletteMode;

    super::draw_console_section_header(ui, "NES", active_system, ActiveSystem::Nes);

    enum_combo(
        ui,
        Id::VideoNesPaletteMode,
        "video_nes_palette_mode",
        None,
        &mut settings.video.nes_palette_mode,
    );
    for id in [Id::VideoLoadNesPaletteFile, Id::VideoClearNesPaletteFile] {
        search::conditional(
            ui,
            id,
            Id::VideoNesPaletteMode,
            settings.video.nes_palette_mode == NesPaletteMode::Custom,
            "Choose Custom palette mode to load or clear a NES palette file.",
        );
    }
    if settings.video.nes_palette_mode == NesPaletteMode::Custom {
        ui.add_space(4.0);
        #[cfg(not(target_arch = "wasm32"))]
        row(
            ui,
            Id::VideoLoadNesPaletteFile,
            Some("Custom palette file"),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut settings.video.nes_custom_palette_path)
                        .hint_text("Path to 192-byte or 1536-byte binary .pal file")
                        .desired_width(240.0),
                )
            },
        );
        #[cfg(target_arch = "wasm32")]
        if settings.video.nes_custom_palette_name.is_empty() {
            ui.monospace("(not uploaded)");
        } else {
            ui.monospace(&settings.video.nes_custom_palette_name);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if row(ui, Id::VideoLoadNesPaletteFile, None, |ui| {
            ui.button("Load .pal...")
        })
        .clicked()
            && let Some(path) = crate::platform::FileDialog::new()
                .add_filter("NES palette", &["pal"])
                .pick_file()
        {
            settings.video.nes_custom_palette_path = path.to_string_lossy().to_string();
            settings.video.nes_custom_palette_name.clear();
            settings.video.nes_custom_palette_bytes.clear();
        }
        #[cfg(target_arch = "wasm32")]
        if row(ui, Id::VideoLoadNesPaletteFile, None, |ui| {
            ui.button("Load .pal...")
        })
        .clicked()
        {
            crate::platform::FileDialog::new()
                .add_filter("NES palette", &["pal"])
                .pick_file_web(nes_palette_file_slot.clone());
        }
        if row(ui, Id::VideoClearNesPaletteFile, None, |ui| {
            ui.button("Clear")
        })
        .clicked()
        {
            settings.video.nes_custom_palette_path.clear();
            settings.video.nes_custom_palette_name.clear();
            settings.video.nes_custom_palette_bytes.clear();
        }

        match nes_palette_status(settings) {
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
    }
}

fn nes_palette_status(settings: &Settings) -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = settings.video.nes_custom_palette_path.trim();
        if path.is_empty() {
            return Err(
                "No custom NES palette file selected; rendering will fall back to raw.".to_string(),
            );
        }
        let bytes =
            std::fs::read(path).map_err(|err| format!("Could not read .pal file: {err}"))?;
        zeff_nes_core::hardware::ppu::parse_nes_palette_bytes(&bytes)
            .map_err(|err| format!("Invalid .pal file: {err}"))?;
        Ok("Valid binary .pal file. 192-byte files provide the base palette; 1536-byte files also provide emphasis groups.".to_string())
    }

    #[cfg(target_arch = "wasm32")]
    {
        if settings.video.nes_custom_palette_bytes.is_empty() {
            return Err(
                "No custom NES palette uploaded; rendering will fall back to raw.".to_string(),
            );
        }
        zeff_nes_core::hardware::ppu::parse_nes_palette_bytes(
            &settings.video.nes_custom_palette_bytes,
        )
        .map_err(|err| format!("Invalid uploaded .pal file: {err}"))?;
        let name = if settings.video.nes_custom_palette_name.is_empty() {
            "uploaded .pal file"
        } else {
            &settings.video.nes_custom_palette_name
        };
        Ok(format!(
            "Using {name}. Browser builds store the uploaded palette bytes in settings."
        ))
    }
}

fn draw_custom_color_matrix(
    ui: &mut egui::Ui,
    id: Id,
    grid_id: &'static str,
    matrix: &mut [f32; 9],
    preset_button_label: Option<&'static str>,
) {
    ui.add_space(4.0);
    let response = ui.label("Custom 3x3 matrix (input RGB -> output RGB)");
    search::target(ui, id, &response);

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

    ui.horizontal_wrapped(|ui| {
        if ui.button("Identity").clicked() {
            *matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        }
        if let Some(label) = preset_button_label
            && ui.button(label).clicked()
        {
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
