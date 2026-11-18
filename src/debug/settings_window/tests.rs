use super::*;

#[test]
fn focused_search_supports_arrows_enter_and_escape() {
    let context = egui::Context::default();
    let mut settings = Settings::default();
    let mut state = DebugWindowState::new();
    state.settings_ui.search = "rewind".into();
    let mut frame = |key: Option<egui::Key>, state: &mut DebugWindowState| {
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("settings_search_field")));
        let events = key
            .map(|key| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .into_iter()
            .collect();
        let _ = context.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| {
                draw_settings_content(
                    ui,
                    &mut settings,
                    state,
                    &SettingsContext {
                        active_system: None,
                        gb_hardware_mode_label: None,
                        is_pocket_camera: false,
                        #[cfg(target_arch = "wasm32")]
                        nes_palette_file_slot: crate::platform::FileDataSlot::default(),
                    },
                )
            },
        );
    };
    frame(None, &mut state);
    frame(Some(egui::Key::ArrowDown), &mut state);
    assert_eq!(state.settings_ui.search_selection, Some(0));
    frame(Some(egui::Key::ArrowDown), &mut state);
    assert_eq!(state.settings_ui.search_selection, Some(1));
    frame(Some(egui::Key::ArrowUp), &mut state);
    assert_eq!(state.settings_ui.search_selection, Some(0));
    let expected = search::results("rewind")[0];
    frame(Some(egui::Key::Enter), &mut state);
    assert!(state.settings_ui.search.is_empty());
    assert_eq!(state.settings_ui.category, expected.category);
    assert_eq!(state.settings_ui.input_page, expected.input_page);
    state.settings_ui.search = "volume".into();
    frame(Some(egui::Key::Escape), &mut state);
    assert!(state.settings_ui.search.is_empty());
    assert_eq!(
        context.memory(|memory| memory.focused()),
        Some(egui::Id::new("settings_search_field"))
    );
}

#[test]
fn interface_reset_preserves_debugger_preferences_and_follows_theme_palette() {
    let mut settings = Settings::default();
    settings.ui.enable_memory_editing = true;
    settings.ui.theme_preset = crate::settings::UiThemePreset::Light;
    settings.ui.debug_colors = crate::settings::DebugColors::for_theme(settings.ui.theme_preset);
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
