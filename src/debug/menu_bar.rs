use crate::debug::dock::{DebugTab, toggle_dock_tab};
use crate::graphics::AspectRatioMode;
use crate::debug::DebugWindowState;
use crate::settings::Settings;
use egui_dock::DockState;
use std::path::PathBuf;

pub(crate) struct MenuActions {
    pub(crate) open_file_requested: bool,
    pub(crate) open_settings_requested: bool,
    pub(crate) save_state_file_requested: bool,
    pub(crate) load_state_file_requested: bool,
    pub(crate) save_state_slot: Option<u8>,
    pub(crate) load_state_slot: Option<u8>,
    pub(crate) aspect_ratio_mode: Option<AspectRatioMode>,
    pub(crate) load_recent_rom: Option<PathBuf>,
    pub(crate) toolbar_settings_changed: bool,
    pub(crate) toggle_fullscreen: bool,
    pub(crate) toggle_pause: bool,
    pub(crate) speed_change: i32,
    pub(crate) start_audio_recording: bool,
    pub(crate) stop_audio_recording: bool,
    pub(crate) start_replay_recording: bool,
    pub(crate) stop_replay_recording: bool,
    pub(crate) load_replay: bool,
    pub(crate) take_screenshot: bool,
    pub(crate) menu_bar_height_points: f32,
    pub(crate) layer_toggles: Option<(bool, bool, bool)>,
}

impl MenuActions {
    pub(crate) fn default(_autohide: bool) -> Self {
        Self {
            open_file_requested: false,
            open_settings_requested: false,
            save_state_file_requested: false,
            load_state_file_requested: false,
            save_state_slot: None,
            load_state_slot: None,
            aspect_ratio_mode: None,
            load_recent_rom: None,
            toolbar_settings_changed: false,
            toggle_fullscreen: false,
            toggle_pause: false,
            speed_change: 0,
            start_audio_recording: false,
            stop_audio_recording: false,
            start_replay_recording: false,
            stop_replay_recording: false,
            load_replay: false,
            take_screenshot: false,
            menu_bar_height_points: 0.0,
            layer_toggles: None,
        }
    }
}

pub(crate) fn draw_menu_bar(
    ctx: &egui::Context,
    current_mode: AspectRatioMode,
    dock_state: &mut DockState<DebugTab>,
    settings: &mut Settings,
    debug_windows: &mut DebugWindowState,
    speed_mode_label: Option<&str>,
    is_recording_audio: bool,
    is_recording_replay: bool,
    is_playing_replay: bool,
    is_paused: bool,
) -> MenuActions {
    let mut open_file_requested = false;
    let mut open_settings_requested = false;
    let mut save_state_file_requested = false;
    let mut load_state_file_requested = false;
    let mut save_state_slot = None;
    let mut load_state_slot = None;
    let mut selected_mode = None;
    let mut load_recent_rom = None;
    let mut toolbar_settings_changed = false;
    let mut toggle_fullscreen = false;
    let mut toggle_pause = false;
    let mut speed_change: i32 = 0;
    let mut start_audio_recording = false;
    let mut stop_audio_recording = false;
    let mut start_replay_recording = false;
    let mut stop_replay_recording = false;
    let mut load_replay = false;
    let mut take_screenshot = false;

    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open").clicked() {
                    open_file_requested = true;
                    ui.close();
                }
                if !settings.recent_roms.is_empty() {
                    ui.menu_button("Recent ROMs", |ui| {
                        let recent = settings.recent_roms.clone();
                        for entry in &recent {
                            let path = std::path::Path::new(&entry.path);
                            let exists = path.exists();
                            let label = if exists {
                                entry.name.clone()
                            } else {
                                format!("{} (missing)", entry.name)
                            };
                            let button = ui.add_enabled(exists, egui::Button::new(label));
                            if button.on_hover_text(&entry.path).clicked() {
                                load_recent_rom = Some(PathBuf::from(&entry.path));
                                ui.close();
                            }
                        }
                    });
                }
                if ui.button("Settings").clicked() {
                    open_settings_requested = true;
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Save State", |ui| {
                    for slot in 1..=4u8 {
                        if ui.button(format!("Slot {slot}")).clicked() {
                            save_state_slot = Some(slot);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Save to File...").clicked() {
                        save_state_file_requested = true;
                        ui.close();
                    }
                });
                ui.menu_button("Load State", |ui| {
                    for slot in 1..=4u8 {
                        if ui.button(format!("Slot {slot}")).clicked() {
                            load_state_slot = Some(slot);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Load from File...").clicked() {
                        load_state_file_requested = true;
                        ui.close();
                    }
                });
                ui.separator();
                if is_recording_audio {
                    if ui.button("⏹ Stop Recording").clicked() {
                        stop_audio_recording = true;
                        ui.close();
                    }
                } else {
                    if ui.button("🎙 Record Audio...").clicked() {
                        start_audio_recording = true;
                        ui.close();
                    }
                }
                ui.separator();
                if is_recording_replay {
                    if ui.button("⏹ Stop Replay Recording").clicked() {
                        stop_replay_recording = true;
                        ui.close();
                    }
                } else if is_playing_replay {
                    ui.label("▶ Replay playing...");
                } else {
                    if ui.button("⏺ Record Replay...").clicked() {
                        start_replay_recording = true;
                        ui.close();
                    }
                    if ui.button("▶ Play Replay...").clicked() {
                        load_replay = true;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Screenshot...").clicked() {
                    take_screenshot = true;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui
                    .selectable_label(current_mode == AspectRatioMode::Stretch, "Stretch")
                    .clicked()
                {
                    selected_mode = Some(AspectRatioMode::Stretch);
                    ui.close();
                }
                if ui
                    .selectable_label(current_mode == AspectRatioMode::KeepAspect, "Keep Aspect")
                    .clicked()
                {
                    selected_mode = Some(AspectRatioMode::KeepAspect);
                    ui.close();
                }
                if ui
                    .selectable_label(
                        current_mode == AspectRatioMode::IntegerScale,
                        "Integer Scale",
                    )
                    .clicked()
                {
                    selected_mode = Some(AspectRatioMode::IntegerScale);
                    ui.close();
                }
                ui.separator();
                if ui.button("Fullscreen (F12)").clicked() {
                    toggle_fullscreen = true;
                    ui.close();
                }
                ui.checkbox(&mut settings.autohide_menu_bar, "Autohide menu bar")
                    .on_hover_text("Hide the menu bar when the cursor is away from the top edge");
                ui.separator();
                ui.menu_button("Shader", |ui| {
                    use crate::settings::ShaderPreset;
                    let presets = [
                        (ShaderPreset::None, "None"),
                        (ShaderPreset::Scanlines, "Scanlines"),
                        (ShaderPreset::LCDGrid, "LCD Grid"),
                        (ShaderPreset::CRT, "CRT"),
                    ];
                    for (preset, label) in presets {
                        if ui.selectable_label(settings.shader_preset == preset, label).clicked() {
                            settings.shader_preset = preset;
                            toolbar_settings_changed = true;
                            ui.close();
                        }
                    }
                    if settings.shader_preset != ShaderPreset::None {
                        ui.separator();
                        let p = &mut settings.shader_params;
                        match settings.shader_preset {
                            ShaderPreset::Scanlines => {
                                ui.add(egui::Slider::new(&mut p.scanline_intensity, 0.0..=1.0).text("Intensity"));
                            }
                            ShaderPreset::LCDGrid => {
                                ui.add(egui::Slider::new(&mut p.grid_intensity, 0.0..=1.0).text("Grid"));
                            }
                            ShaderPreset::CRT => {
                                ui.add(egui::Slider::new(&mut p.scanline_intensity, 0.0..=1.0).text("Scanlines"));
                                ui.add(egui::Slider::new(&mut p.crt_curvature, 0.0..=1.0).text("Curvature"));
                            }
                            ShaderPreset::None => {}
                        }
                    }
                });
            });

            ui.menu_button("Debug", |ui| {
                if !crate::debug::has_game_view_tab(dock_state) {
                    if ui.button("Show Game View").clicked() {
                        crate::debug::ensure_game_view_tab(dock_state);
                        ui.close();
                    }
                    ui.separator();
                }
                if ui.button("CPU / Debug").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::CpuDebug);
                    ui.close();
                }
                if ui.button("Input").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::InputViewer);
                    ui.close();
                }
                if ui.button("APU / Sound").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::ApuViewer);
                    ui.close();
                }
                if ui.button("ROM Info").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::RomInfo);
                    ui.close();
                }
                if ui.button("Disassembler").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::Disassembler);
                    ui.close();
                }
                if ui.button("Memory Viewer").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::MemoryViewer);
                    ui.close();
                }
                if ui.button("ROM Viewer").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::RomViewer);
                    ui.close();
                }
                if ui.button("Tile Data").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::TileViewer);
                    ui.close();
                }
                if ui.button("Tile Map").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::TilemapViewer);
                    ui.close();
                }
                if ui.button("OAM / Sprites").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::OamViewer);
                    ui.close();
                }
                if ui.button("Palettes").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::PaletteViewer);
                    ui.close();
                }
                ui.separator();
                if ui.button("Performance").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::Performance);
                    ui.close();
                }
                if ui.button("Breakpoints").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::Breakpoints);
                    ui.close();
                }
                if ui.button("Cheats").clicked() {
                    toggle_dock_tab(dock_state, DebugTab::Cheats);
                    ui.close();
                }
                ui.separator();
                ui.label("PPU Layers");
                ui.checkbox(&mut debug_windows.layer_enable_bg, "Background");
                ui.checkbox(&mut debug_windows.layer_enable_window, "Window");
                ui.checkbox(&mut debug_windows.layer_enable_sprites, "Sprites");
                ui.separator();
                if ui.button("Reset Layout (Floating)").clicked() {
                    *dock_state = crate::debug::create_default_dock_state();
                    ui.close();
                }
                if ui.button("Reset Layout (IDE)").clicked() {
                    *dock_state = crate::debug::create_ide_dock_state();
                    ui.close();
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("GitHub Repository").clicked() {
                    let _ = open::that("https://github.com/zeffuro/zeff-boy");
                    ui.close();
                }
                if ui.button("Open Settings Folder").clicked() {
                    let dir = Settings::settings_dir();
                    let _ = open::that(&dir);
                    ui.close();
                }
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let pause_icon = if is_paused { "▶" } else { "⏸" };
                let pause_tooltip = if is_paused { "Resume (F9)" } else { "Pause (F9)" };
                if ui.small_button(pause_icon).on_hover_text(pause_tooltip).clicked() {
                    toggle_pause = true;
                }

                ui.separator();

                let mult = settings.fast_forward_multiplier;
                if ui.small_button("+").on_hover_text("Increase speed multiplier").clicked() {
                    speed_change = 1;
                }
                ui.label(
                    egui::RichText::new(format!("{}×", mult))
                        .small()
                        .color(egui::Color32::LIGHT_GRAY),
                );
                if ui.small_button("−").on_hover_text("Decrease speed multiplier").clicked() {
                    speed_change = -1;
                }

                ui.separator();

                if let Some(label) = speed_mode_label {
                    ui.label(
                        egui::RichText::new(label)
                            .small()
                            .color(egui::Color32::LIGHT_GRAY),
                    );
                    ui.separator();
                }

                let muted = settings.master_volume <= 0.001;
                let icon = if muted { "🔇" } else { "🔊" };
                if ui.small_button(icon).clicked() {
                    if muted {
                        settings.master_volume =
                            settings.pre_mute_volume.take().unwrap_or(1.0);
                    } else {
                        settings.pre_mute_volume = Some(settings.master_volume);
                        settings.master_volume = 0.0;
                    }
                    toolbar_settings_changed = true;
                }

                let vol_before = settings.master_volume;
                ui.spacing_mut().slider_width = 80.0;
                ui.add(
                    egui::Slider::new(&mut settings.master_volume, 0.0..=1.0)
                        .show_value(false)
                        .text(""),
                );
                if (settings.master_volume - vol_before).abs() > f32::EPSILON {
                    toolbar_settings_changed = true;
                }
            });
        });
    });

    MenuActions {
        open_file_requested,
        open_settings_requested,
        save_state_file_requested,
        load_state_file_requested,
        save_state_slot,
        load_state_slot,
        aspect_ratio_mode: selected_mode,
        load_recent_rom,
        toolbar_settings_changed,
        toggle_fullscreen,
        toggle_pause,
        speed_change,
        start_audio_recording,
        stop_audio_recording,
        start_replay_recording,
        stop_replay_recording,
        load_replay,
        take_screenshot,
        menu_bar_height_points: ctx.available_rect().min.y.max(0.0),
        layer_toggles: Some((
            debug_windows.layer_enable_bg,
            debug_windows.layer_enable_window,
            debug_windows.layer_enable_sprites,
        )),
    }
}

