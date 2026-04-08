mod debug_menu;
mod file_menu;
mod toolbar;
mod tools_menu;
mod view_menu;

use crate::debug::DebugWindowState;
use crate::debug::dock::DebugTab;
use crate::graphics::AspectRatioMode;
use crate::settings::Settings;
use egui_dock::DockState;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum MenuAction {
    OpenFile,
    ResetGame,
    StopGame,
    OpenSettings,
    SaveStateFile,
    LoadStateFile,
    SaveStateSlot(u8),
    LoadStateSlot(u8),
    SetAspectRatio(AspectRatioMode),
    LoadRecentRom(PathBuf),
    ToolbarSettingsChanged,
    ToggleFullscreen,
    TogglePause,
    SpeedChange(i32),
    StartAudioRecording,
    StopAudioRecording,
    StartReplayRecording,
    StopReplayRecording,
    LoadReplay,
    TakeScreenshot,
    SetLayerToggles(bool, bool, bool),
}

pub(crate) struct MenuBarResult {
    pub(crate) actions: Vec<MenuAction>,
    pub(crate) menu_bar_height_points: f32,
}

impl MenuBarResult {
    pub(crate) fn empty() -> Self {
        Self {
            actions: Vec::new(),
            menu_bar_height_points: 0.0,
        }
    }
}

pub(crate) struct MenuBarContext<'a> {
    pub(crate) current_mode: AspectRatioMode,
    pub(crate) speed_mode_label: Option<&'a str>,
    pub(crate) is_recording_audio: bool,
    pub(crate) is_recording_replay: bool,
    pub(crate) is_playing_replay: bool,
    pub(crate) is_paused: bool,
    pub(crate) slot_labels: &'a [String; 10],
    pub(crate) slot_occupied: &'a [bool; 10],
    pub(crate) active_save_slot: u8,
}

pub(crate) fn draw_menu_bar(
    ctx: &egui::Context,
    mb: &MenuBarContext<'_>,
    dock_state: &mut DockState<DebugTab>,
    settings: &mut Settings,
    debug_windows: &mut DebugWindowState,
) -> MenuBarResult {
    let mut actions = Vec::new();
    let menu_bar_height_points = egui::Area::new(egui::Id::new("menu_bar"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_width(ctx.content_rect().width());
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .stroke(egui::Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    egui::MenuBar::new().ui(ui, |ui| {
                        ui.menu_button("File", |ui| {
                            file_menu::draw(
                                ui,
                                &mut actions,
                                settings,
                                &file_menu::FileMenuState {
                                    slot_labels: mb.slot_labels,
                                    slot_occupied: mb.slot_occupied,
                                    active_slot: mb.active_save_slot,
                                    is_recording_audio: mb.is_recording_audio,
                                    is_recording_replay: mb.is_recording_replay,
                                    is_playing_replay: mb.is_playing_replay,
                                },
                            );
                        });

                        ui.menu_button("View", |ui| {
                            view_menu::draw(ui, &mut actions, settings, mb.current_mode);
                        });

                        ui.menu_button("Debug", |ui| {
                            debug_menu::draw(ui, dock_state);
                        });

                        ui.menu_button("Tools", |ui| {
                            tools_menu::draw(ui, dock_state, debug_windows);
                        });

                        ui.menu_button("Help", |ui| {
                            ui.label(format!("zeff-boy v{}", env!("CARGO_PKG_VERSION")));
                            ui.separator();
                            if ui.button("GitHub Repository").clicked() {
                                crate::platform::open_url("https://github.com/zeffuro/zeff-boy");
                                ui.close();
                            }
                            if ui.button("Open Settings Folder").clicked() {
                                let dir = Settings::settings_dir();
                                crate::platform::open_url(&dir.display().to_string());
                                ui.close();
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            toolbar::draw(
                                ui,
                                &mut actions,
                                settings,
                                mb.is_paused,
                                mb.speed_mode_label,
                                mb.active_save_slot,
                            );
                        });
                    });
                });

            ui.min_rect().height()
        })
        .inner;

    actions.push(MenuAction::SetLayerToggles(
        debug_windows.layer_enable_bg,
        debug_windows.layer_enable_window,
        debug_windows.layer_enable_sprites,
    ));

    MenuBarResult {
        actions,
        menu_bar_height_points,
    }
}
