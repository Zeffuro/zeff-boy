mod audio;
mod camera;
mod controls;
mod debugger;
mod emulation;
mod firmware;
mod general;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod render_fixtures;
mod search;
mod storage;
mod ui;
mod video;

use crate::debug::DebugWindowState;
use crate::emu_backend::ActiveSystem;
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SettingsCategory {
    #[default]
    General,
    InputDevices,
    Audio,
    Video,
    Emulation,
    Firmware,
    Storage,
    Camera,
    Interface,
    Debugger,
}

impl SettingsCategory {
    const ALL: [Self; 10] = [
        Self::General,
        Self::InputDevices,
        Self::Audio,
        Self::Video,
        Self::Emulation,
        Self::Firmware,
        Self::Storage,
        Self::Camera,
        Self::Interface,
        Self::Debugger,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::InputDevices => "Input & Devices",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Emulation => "Emulation",
            Self::Firmware => "Firmware",
            Self::Storage => "Storage & Recovery",
            Self::Camera => "Camera",
            Self::Interface => "Interface & Accessibility",
            Self::Debugger => "Debugger",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum InputDevicesPage {
    #[default]
    Controls,
    Hotkeys,
    TestAndCalibrate,
}

impl InputDevicesPage {
    const ALL: [Self; 3] = [Self::Controls, Self::Hotkeys, Self::TestAndCalibrate];

    fn label(self) -> &'static str {
        match self {
            Self::Controls => "Controls",
            Self::Hotkeys => "Hotkeys",
            Self::TestAndCalibrate => "Test & Calibrate",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfileAction {
    Update(String),
    Reset(String),
}

#[derive(Clone, Copy)]
enum PlayerResetTarget {
    Keyboard(u8),
    Gamepad(u8),
    WonderSwanKeyboard,
    WonderSwanGamepad,
    WonderSwanClear,
}

#[derive(Clone, Copy)]
enum ResetTarget {
    Page(SettingsCategory),
    Preferences,
}

pub(crate) struct SettingsUiState {
    category: SettingsCategory,
    pub(crate) search: String,
    pub(crate) input_page: InputDevicesPage,
    pub(crate) selected_player: u8,
    pub(crate) controller_layout: Option<controls::controller_diagram::DiagramKind>,
    last_controller_layout: Option<controls::controller_diagram::DiagramKind>,
    pub(crate) binding_source: controls::BindingSource,
    pub(crate) selected_profile_id: Option<String>,
    pub(crate) profile_name: String,
    search_jump: bool,
    test_device: Option<crate::input::RuntimeGamepadId>,
    profile_notice: Option<String>,
    profile_delete_confirmation: Option<String>,
    profile_action_confirmation: Option<ProfileAction>,
    player_reset_confirmation: Option<PlayerResetTarget>,
    reset_confirmation: Option<ResetTarget>,
    undo: Option<Settings>,
    undo_label: Option<String>,
    undo_baseline: Option<Settings>,
    pub(crate) gamepad_snapshot: crate::input::GamepadSnapshot,
    pub(crate) gamepad_commands: Vec<crate::input::GamepadCommand>,
}

impl Default for SettingsUiState {
    fn default() -> Self {
        Self {
            category: SettingsCategory::default(),
            search: String::new(),
            input_page: InputDevicesPage::default(),
            selected_player: 1,
            controller_layout: None,
            last_controller_layout: None,
            binding_source: controls::BindingSource::default(),
            selected_profile_id: None,
            profile_name: String::new(),
            search_jump: false,
            test_device: None,
            profile_notice: None,
            profile_delete_confirmation: None,
            profile_action_confirmation: None,
            player_reset_confirmation: None,
            reset_confirmation: None,
            undo: None,
            undo_label: None,
            undo_baseline: None,
            gamepad_snapshot: crate::input::GamepadSnapshot::default(),
            gamepad_commands: Vec::new(),
        }
    }
}
impl SettingsUiState {
    pub(crate) fn wants_live_input(&self) -> bool {
        self.category == SettingsCategory::InputDevices && self.search.trim().is_empty()
    }
}
pub(crate) struct SettingsContext<'a> {
    pub active_system: Option<ActiveSystem>,
    pub gb_hardware_mode_label: Option<&'a str>,
    pub is_pocket_camera: bool,
    #[cfg(target_arch = "wasm32")]
    pub nes_palette_file_slot: crate::platform::FileDataSlot,
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    open: &mut bool,
    constrain_rect: egui::Rect,
    emu: &SettingsContext<'_>,
) {
    egui::Window::new("Settings")
        .open(open)
        .default_width(720.0)
        .default_height(560.0)
        .resizable(true)
        .constrain_to(constrain_rect)
        .show(ctx, |ui| {
            draw_settings_content(ui, settings, state, emu);
        });
}

pub(crate) fn draw_settings_content(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    emu: &SettingsContext<'_>,
) {
    ui.spacing_mut().item_spacing.y = 6.0;
    draw_persistence_notice(ui, settings);
    let previous_category = state.settings_ui.category;
    invalidate_stale_undo(settings, &mut state.settings_ui);
    {
        let settings_ui = &mut state.settings_ui;
        if draw_undo(ui, settings, settings_ui) {
            controls::cancel_capture(state);
        }
    }

    let narrow = ui.available_width() < 620.0;
    if narrow {
        draw_compact_navigation(ui, &mut state.settings_ui);
        ui.separator();
        draw_content(ui, settings, state, emu);
    } else {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(190.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| draw_sidebar(ui, &mut state.settings_ui),
            );
            ui.separator();
            ui.vertical(|ui| draw_content(ui, settings, state, emu));
        });
    }
    if previous_category != state.settings_ui.category
        || !state.settings_ui.search.trim().is_empty()
    {
        controls::cancel_capture(state);
    }
}

fn draw_persistence_notice(ui: &mut egui::Ui, settings: &Settings) {
    if let Some(notice) = settings.persistence_notice() {
        ui.group(|ui| {
            ui.label(egui::RichText::new(notice).color(egui::Color32::YELLOW));
            if ui.small_button("Retry save").clicked() {
                settings.save();
            }
        });
        ui.add_space(4.0);
    }
}

fn invalidate_stale_undo(settings: &Settings, settings_ui: &mut SettingsUiState) {
    if settings_ui
        .undo_baseline
        .as_ref()
        .is_some_and(|baseline| baseline != settings)
    {
        settings_ui.undo = None;
        settings_ui.undo_label = None;
        settings_ui.undo_baseline = None;
    }
}
fn draw_undo(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
) -> bool {
    let mut restored = false;
    if let Some(label) = settings_ui.undo_label.clone() {
        ui.horizontal(|ui| {
            ui.label(label);
            if ui.button("Undo").clicked() {
                if let Some(previous) = settings_ui.undo.take() {
                    *settings = previous;
                    restored = true;
                }
                settings_ui.undo_label = None;
                settings_ui.undo_baseline = None;
            }
            if ui.small_button("Dismiss").clicked() {
                settings_ui.undo = None;
                settings_ui.undo_label = None;
                settings_ui.undo_baseline = None;
            }
        });
        ui.separator();
    }
    restored
}

fn draw_search_field(ui: &mut egui::Ui, settings_ui: &mut SettingsUiState) {
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 30.0).max(100.0);
        ui.add(
            egui::TextEdit::singleline(&mut settings_ui.search)
                .hint_text("Search settings…")
                .desired_width(width),
        );
        if ui
            .add_enabled(!settings_ui.search.is_empty(), egui::Button::new("×"))
            .clicked()
        {
            settings_ui.search.clear();
        }
    });
}

fn draw_sidebar(ui: &mut egui::Ui, settings_ui: &mut SettingsUiState) {
    ui.heading("Settings");
    draw_search_field(ui, settings_ui);
    ui.add_space(6.0);
    for category in SettingsCategory::ALL {
        if ui
            .add_sized(
                [ui.available_width(), 32.0],
                egui::Button::selectable(
                    settings_ui.search.is_empty() && settings_ui.category == category,
                    category.label(),
                ),
            )
            .clicked()
        {
            settings_ui.category = category;
            settings_ui.search.clear();
            settings_ui.search_jump = true;
        }
    }
}

fn draw_compact_navigation(ui: &mut egui::Ui, settings_ui: &mut SettingsUiState) {
    ui.horizontal_wrapped(|ui| {
        ui.heading("Settings");
        egui::ComboBox::from_id_salt("settings_category")
            .selected_text(settings_ui.category.label())
            .show_ui(ui, |ui| {
                for category in SettingsCategory::ALL {
                    if ui
                        .selectable_value(&mut settings_ui.category, category, category.label())
                        .changed()
                    {
                        settings_ui.search.clear();
                        settings_ui.search_jump = true;
                    }
                }
            });
    });
    draw_search_field(ui, settings_ui);
}

fn draw_search_results(ui: &mut egui::Ui, state: &mut DebugWindowState) {
    let results = search::results(&state.settings_ui.search);
    ui.heading("Search results");
    ui.label(egui::RichText::new(format!("{} matching settings", results.len())).weak());
    ui.label(
        egui::RichText::new("Select a result to open its settings page.")
            .small()
            .weak(),
    );
    ui.add_space(8.0);
    if results.is_empty() {
        ui.label("No settings found. Try a setting name, system, or action.");
    }
    for entry in results {
        let response = egui::Frame::group(ui.style())
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.strong(entry.title);
                let location = if entry.category == SettingsCategory::InputDevices {
                    format!("{} / {}", entry.category.label(), entry.input_page.label())
                } else {
                    entry.category.label().to_owned()
                };
                ui.label(egui::RichText::new(location).small().weak());
            })
            .response
            .interact(egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            state.settings_ui.category = entry.category;
            state.settings_ui.input_page = entry.input_page;
            state.settings_ui.search.clear();
            state.settings_ui.search_jump = true;
            controls::cancel_capture(state);
            break;
        }
    }
}

fn draw_content(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    emu: &SettingsContext<'_>,
) {
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt((
            "settings_page",
            state.settings_ui.category as u8,
            state.settings_ui.input_page as u8,
        ))
        .auto_shrink(false);
    if std::mem::take(&mut state.settings_ui.search_jump) {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    scroll.show(ui, |ui| {
        if state.settings_ui.search.trim().is_empty() {
            draw_content_body(ui, settings, state, emu);
        } else {
            draw_search_results(ui, state);
        }
    });
}

fn draw_content_body(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    emu: &SettingsContext<'_>,
) {
    let category = state.settings_ui.category;
    ui.horizontal(|ui| {
        ui.heading(category.label());
        ui.label(egui::RichText::new("Global defaults").weak().small());
    });
    ui.separator();

    match category {
        SettingsCategory::General => general::draw(ui, settings),
        SettingsCategory::InputDevices => controls::draw(ui, settings, state, emu.active_system),
        SettingsCategory::Audio => audio::draw(ui, settings),
        SettingsCategory::Video => video::draw(
            ui,
            settings,
            emu.active_system,
            emu.gb_hardware_mode_label,
            emu.is_pocket_camera,
            #[cfg(target_arch = "wasm32")]
            emu.nes_palette_file_slot.clone(),
        ),
        SettingsCategory::Emulation => emulation::draw(ui, settings, emu.active_system),
        SettingsCategory::Firmware => firmware::draw(ui, settings, state),
        SettingsCategory::Storage => storage::draw(ui, settings),
        SettingsCategory::Camera => camera::draw(ui, settings, state),
        SettingsCategory::Interface => ui::draw(ui, settings),
        SettingsCategory::Debugger => debugger::draw(ui, settings),
    }

    ui.separator();
    if draw_reset_controls(ui, settings, &mut state.settings_ui, category) {
        controls::cancel_capture(state);
    }
}

fn draw_reset_controls(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_ui: &mut SettingsUiState,
    category: SettingsCategory,
) -> bool {
    let mut reset = false;
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_sized([180.0, 30.0], egui::Button::new("Reset this page…"))
            .clicked()
        {
            settings_ui.reset_confirmation = Some(ResetTarget::Page(category));
        }
        if ui
            .add_sized([180.0, 30.0], egui::Button::new("Reset all preferences…"))
            .clicked()
        {
            settings_ui.reset_confirmation = Some(ResetTarget::Preferences);
        }
    });

    let Some(target) = settings_ui.reset_confirmation else {
        return false;
    };
    ui.group(|ui| {
        let label = match target {
            ResetTarget::Page(SettingsCategory::InputDevices) => {
                "Reset current input mappings and player assignments? Saved input profiles will remain.".to_string()
            }
            ResetTarget::Page(category) => format!("Reset the {} page?", category.label()),
            ResetTarget::Preferences => "Reset all saved preferences?".to_string(),
        };
        ui.label(label);
        ui.label(
            egui::RichText::new("Undo is available until another settings change is made.")
                .small()
                .weak(),
        );
        ui.horizontal(|ui| {
            if ui.button("Reset").clicked() {
                reset = true;
                let previous = settings.clone();
                match target {
                    ResetTarget::Page(category) => reset_page(settings, category),
                    ResetTarget::Preferences => settings.reset_preferences(),
                }
                settings_ui.undo = Some(previous);
                settings_ui.undo_label = Some("Settings reset.".to_string());
                settings_ui.undo_baseline = Some(settings.clone());
                settings_ui.reset_confirmation = None;
            }
            if ui.button("Cancel").clicked() {
                settings_ui.reset_confirmation = None;
            }
        });
    });
    reset
}

fn reset_page(settings: &mut Settings, category: SettingsCategory) {
    let defaults = Settings::default();
    match category {
        SettingsCategory::General => {
            settings.emulation.pause_on_unfocus = defaults.emulation.pause_on_unfocus;
            settings.ui.check_for_updates = defaults.ui.check_for_updates;
            settings.recent_roms.clear();
        }
        SettingsCategory::InputDevices => {
            let bindings = defaults.capture_input_profile();
            settings.apply_input_profile(&bindings);
            settings.input_devices = defaults.input_devices;
        }
        SettingsCategory::Audio => settings.audio = defaults.audio,
        SettingsCategory::Video => settings.video = defaults.video,
        SettingsCategory::Emulation => {
            settings.emulation.hardware_mode_preference =
                defaults.emulation.hardware_mode_preference;
            settings.emulation.sega8_video_standard = defaults.emulation.sega8_video_standard;
            settings.emulation.sega8_console_region = defaults.emulation.sega8_console_region;
            settings.emulation.pce_console_wiring = defaults.emulation.pce_console_wiring;
            settings.emulation.pce_controller = defaults.emulation.pce_controller;
            settings.emulation.pce_memory_base = defaults.emulation.pce_memory_base;
            settings.emulation.pce_arcade_card = defaults.emulation.pce_arcade_card;
            settings.emulation.pce_mouse_sensitivity = defaults.emulation.pce_mouse_sensitivity;
            settings.emulation.pce_mouse_cursor_mode = defaults.emulation.pce_mouse_cursor_mode;
            settings.emulation.pce_cd_archive_memory_limit =
                defaults.emulation.pce_cd_archive_memory_limit;
            settings.emulation.fast_forward_multiplier = defaults.emulation.fast_forward_multiplier;
            settings.emulation.slow_motion_divisor = defaults.emulation.slow_motion_divisor;
            settings.emulation.slow_motion_enabled = defaults.emulation.slow_motion_enabled;
            settings.emulation.uncapped_frames_per_tick =
                defaults.emulation.uncapped_frames_per_tick;
            settings.emulation.uncapped_speed = defaults.emulation.uncapped_speed;
            settings.emulation.frame_skip = defaults.emulation.frame_skip;
            settings.emulation.sgb_border_enabled = defaults.emulation.sgb_border_enabled;
            settings.emulation.nes_zapper_enabled = defaults.emulation.nes_zapper_enabled;
            settings.emulation.tcp_link_addr = defaults.emulation.tcp_link_addr;
        }
        SettingsCategory::Firmware => {
            settings.emulation.firmware_directory = defaults.emulation.firmware_directory;
            settings.emulation.gb_boot_rom_mode = defaults.emulation.gb_boot_rom_mode;
            settings.emulation.sega_boot_rom_mode = defaults.emulation.sega_boot_rom_mode;
            settings.emulation.gba_bios_mode = defaults.emulation.gba_bios_mode;
        }
        SettingsCategory::Storage => {
            settings.emulation.save_recovery_state = defaults.emulation.save_recovery_state;
            settings.emulation.resume_recovery_state = defaults.emulation.resume_recovery_state;
            settings.emulation.recovery_migration_notice_pending = false;
        }
        SettingsCategory::Camera => settings.camera = defaults.camera,
        SettingsCategory::Interface => {
            if settings.ui.debug_colors
                == crate::settings::DebugColors::for_theme(settings.ui.theme_preset)
            {
                settings.ui.debug_colors =
                    crate::settings::DebugColors::for_theme(defaults.ui.theme_preset);
            }
            settings.ui.theme_preset = defaults.ui.theme_preset;
            settings.ui.ui_density = defaults.ui.ui_density;
            settings.ui.ui_scale = defaults.ui.ui_scale;
            settings.ui.autohide_menu_bar = defaults.ui.autohide_menu_bar;
        }
        SettingsCategory::Debugger => {
            settings.ui.debug_colors = defaults.ui.debug_colors;
            settings.ui.debug_monospace_scale = defaults.ui.debug_monospace_scale;
            settings.ui.debug_presentation = defaults.ui.debug_presentation;
            settings.ui.enable_memory_editing = defaults.ui.enable_memory_editing;
            settings.ui.show_fps = defaults.ui.show_fps;
        }
    }
}
pub(super) fn draw_console_section_header(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_reset_preserves_debugger_preferences_and_follows_theme_palette() {
        let mut settings = Settings::default();
        settings.ui.enable_memory_editing = true;
        settings.ui.theme_preset = crate::settings::UiThemePreset::Light;
        settings.ui.debug_colors =
            crate::settings::DebugColors::for_theme(settings.ui.theme_preset);
        reset_page(&mut settings, SettingsCategory::Interface);
        assert!(settings.ui.enable_memory_editing);
        assert_eq!(
            settings.ui.debug_colors,
            crate::settings::DebugColors::for_theme(settings.ui.theme_preset)
        );
        settings.ui.debug_colors.address = [1, 2, 3, 255];
        let custom = settings.ui.debug_colors;
        reset_page(&mut settings, SettingsCategory::Interface);
        assert_eq!(settings.ui.debug_colors, custom);
        reset_page(&mut settings, SettingsCategory::Debugger);
        assert_eq!(
            settings.ui.enable_memory_editing,
            Settings::default().ui.enable_memory_editing
        );
    }

    #[test]
    fn controller_layout_views_preserve_default_mappings_and_automatic_assignments() {
        let context = egui::Context::default();
        let mut settings = Settings::default();
        let defaults = settings.clone();
        let mut state = DebugWindowState::new();
        state.settings_ui.category = SettingsCategory::InputDevices;
        for layout in controls::controller_diagram::DiagramKind::ALL {
            for source in [
                controls::BindingSource::Controller,
                controls::BindingSource::Keyboard,
            ] {
                state.settings_ui.controller_layout = Some(layout);
                state.settings_ui.binding_source = source;
                render_settings_frame(&context, 1000.0, &mut settings, &mut state);
                assert_eq!(
                    settings, defaults,
                    "Changing the visual layout/source must not rewrite working mappings"
                );
            }
        }
    }

    fn render_settings_frame(
        context: &egui::Context,
        width: f32,
        settings: &mut Settings,
        state: &mut DebugWindowState,
    ) -> egui::FullOutput {
        crate::graphics::apply_egui_theme(
            context,
            settings.ui.theme_preset,
            settings.ui.ui_density,
            settings.ui.debug_monospace_scale,
            settings.ui.effective_debug_colors(),
        );
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 640.0),
                )),
                ..egui::RawInput::default()
            },
            |ui| {
                draw_settings_content(
                    ui,
                    settings,
                    state,
                    &SettingsContext {
                        active_system: None,
                        gb_hardware_mode_label: None,
                        is_pocket_camera: false,
                        #[cfg(target_arch = "wasm32")]
                        nes_palette_file_slot: crate::platform::FileDataSlot::default(),
                    },
                );
            },
        )
    }

    #[test]
    fn settings_content_renders_at_compact_and_sidebar_widths() {
        let context = egui::Context::default();
        let mut settings = Settings::default();
        let mut state = DebugWindowState::new();

        let compact = render_settings_frame(&context, 480.0, &mut settings, &mut state);
        assert!(!compact.shapes.is_empty());

        state.settings_ui.search = "input".to_owned();
        let sidebar = render_settings_frame(&context, 900.0, &mut settings, &mut state);
        assert!(!sidebar.shapes.is_empty());
        assert!(!search::results(&state.settings_ui.search).is_empty());
    }

    #[test]
    fn input_page_reset_restores_current_bindings_and_preserves_saved_profiles() {
        let mut settings = Settings::default();
        settings.input_devices.players[0] = crate::settings::GamepadAssignment::Disabled;
        settings.save_input_profile("My controls").unwrap();
        let saved_profiles = settings.input_profiles.clone();

        reset_page(&mut settings, SettingsCategory::InputDevices);

        assert_eq!(settings.input_devices, Settings::default().input_devices);
        assert_eq!(
            settings.capture_input_profile(),
            Settings::default().capture_input_profile()
        );
        assert_eq!(settings.input_profiles, saved_profiles);
    }

    #[test]
    fn undo_is_discarded_after_an_unrelated_later_change() {
        let mut settings = Settings::default();
        let original = settings.clone();
        settings.emulation.pause_on_unfocus = !settings.emulation.pause_on_unfocus;
        let mut settings_ui = SettingsUiState {
            undo: Some(original),
            undo_label: Some("Settings reset.".to_owned()),
            undo_baseline: Some(settings.clone()),
            ..SettingsUiState::default()
        };

        settings.ui.check_for_updates = !settings.ui.check_for_updates;
        invalidate_stale_undo(&settings, &mut settings_ui);

        assert!(settings_ui.undo.is_none());
        assert!(settings_ui.undo_label.is_none());
        assert!(settings_ui.undo_baseline.is_none());
    }
}
