mod debug_menu;
mod file_menu;
mod toolbar;
mod tools_menu;
mod view_menu;

use crate::debug::DebugWindowState;
use crate::debug::dock::DebugTab;
use crate::emu_backend::ActiveSystem;
use crate::graphics::AspectRatioMode;
use crate::settings::DebugPresentation;
use crate::settings::Settings;
use egui_dock::DockState;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum MenuAction {
    OpenFile,
    ResetGame,
    StopGame,
    OpenSettings,
    OpenPrinterWindow,
    LoadSymbolFile,
    SetDebugPresentation(DebugPresentation),
    OpenDebuggerWindow,
    SaveStateFile,
    LoadStateFile,
    UndoLoadState,
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
    ApplyMediaEvent(zeff_emu_common::media::MediaEvent),
    SetGameBoySerialDevice(zeff_gb_core::hardware::GameBoySerialDevice),
    #[cfg(not(target_arch = "wasm32"))]
    ScanBardigunBarcodeFile,
    OpenBarcodeBoyScan,
    TriggerBarcodeBoyScan(String),
    HostTcpLink,
    JoinTcpLink,
    DisconnectLink,
    ToggleWsRotation,
    SetLayerToggles(bool, bool, bool),
    SetGbaBgLayerToggles([bool; 4]),
    #[cfg(not(target_arch = "wasm32"))]
    CheckForUpdates,
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
    pub(crate) supports_save_states: bool,
    pub(crate) supports_replay: bool,
    pub(crate) supports_audio: bool,
    pub(crate) supports_debugger: bool,
    pub(crate) is_paused: bool,
    pub(crate) active_system: ActiveSystem,
    pub(crate) media_slot_snapshot: Option<&'a zeff_emu_common::media::MediaSlotSnapshot>,
    pub(crate) media_event_change_allowed: bool,
    pub(crate) game_boy_serial_device: zeff_gb_core::hardware::GameBoySerialDevice,
    pub(crate) game_boy_serial_device_change_allowed: bool,
    pub(crate) ws_display_rotated: bool,
    pub(crate) slot_labels: &'a [String; 10],
    pub(crate) slot_occupied: &'a [bool; 10],
    pub(crate) active_save_slot: u8,
    pub(crate) can_undo_load_state: bool,
    pub(crate) external_debugger: bool,
    pub(crate) debugger_window_open: bool,
    pub(crate) debug_presentation: DebugPresentation,
}

pub(crate) fn draw_menu_bar(
    root_ui: &mut egui::Ui,
    mb: &MenuBarContext<'_>,
    dock_state: &mut DockState<DebugTab>,
    settings: &mut Settings,
    debug_windows: &mut DebugWindowState,
) -> MenuBarResult {
    let mut actions = Vec::new();
    let menu_bar_height_points = egui::Panel::top("menu_bar")
        .frame(
            egui::Frame::new()
                .fill(root_ui.visuals().faint_bg_color)
                .stroke(egui::Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(6, 4)),
        )
        .show(root_ui, |ui| {
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
                            can_undo_load_state: mb.can_undo_load_state,
                            is_recording_audio: mb.is_recording_audio,
                            is_recording_replay: mb.is_recording_replay,
                            is_playing_replay: mb.is_playing_replay,
                            supports_save_states: mb.supports_save_states,
                            supports_replay: mb.supports_replay,
                            supports_audio: mb.supports_audio,
                        },
                    );
                });

                ui.menu_button("View", |ui| {
                    view_menu::draw(ui, &mut actions, settings, mb.current_mode);
                });

                ui.menu_button("Debug", |ui| {
                    debug_menu::draw(
                        ui,
                        &mut actions,
                        dock_state,
                        debug_menu::DebugMenuState {
                            external_debugger: mb.external_debugger,
                            debugger_window_open: mb.debugger_window_open,
                            presentation: mb.debug_presentation,
                            supports_debugger: mb.supports_debugger,
                        },
                    );
                });

                egui::containers::menu::MenuButton::new("Tools")
                    .config(
                        egui::containers::menu::MenuConfig::new()
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                    )
                    .ui(ui, |ui| {
                        tools_menu::draw(
                            ui,
                            &mut actions,
                            dock_state,
                            debug_windows,
                            tools_menu::ToolsMenuState {
                                active_system: mb.active_system,
                                media_slot_snapshot: mb.media_slot_snapshot,
                                media_event_change_allowed: mb.media_event_change_allowed,
                                game_boy_serial_device: mb.game_boy_serial_device,
                                game_boy_serial_device_change_allowed: mb
                                    .game_boy_serial_device_change_allowed,
                            },
                        );
                    });

                ui.menu_button("Help", |ui| {
                    ui.label(format!("zeff-boy v{}", env!("CARGO_PKG_VERSION")));
                    ui.separator();
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Check for Updates").clicked() {
                        actions.push(MenuAction::CheckForUpdates);
                        ui.close();
                    }
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
                        toolbar::ToolbarState {
                            is_paused: mb.is_paused,
                            active_system: mb.active_system,
                            ws_display_rotated: mb.ws_display_rotated,
                            speed_mode_label: mb.speed_mode_label,
                            active_save_slot: mb.active_save_slot,
                        },
                    );
                });
            });
        })
        .response
        .rect
        .height();

    actions.push(MenuAction::SetLayerToggles(
        debug_windows.layer_enable_bg,
        debug_windows.layer_enable_window,
        debug_windows.layer_enable_sprites,
    ));
    actions.push(MenuAction::SetGbaBgLayerToggles(
        debug_windows.gba_layer_enable_bg,
    ));

    MenuBarResult {
        actions,
        menu_bar_height_points,
    }
}
