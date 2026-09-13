use super::*;

#[test]
#[ignore = "manual GPU-backed settings layout captures; requires an output directory"]
fn render_settings_fixtures() -> anyhow::Result<()> {
    let output = std::env::var_os("ZEFF_SETTINGS_CAPTURE_DIR").ok_or_else(|| {
        anyhow::anyhow!("set ZEFF_SETTINGS_CAPTURE_DIR before rendering fixtures")
    })?;
    let output = std::path::PathBuf::from(output);
    std::fs::create_dir_all(&output)?;
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

    for (name, category, input_page, width, height, scale) in [
        (
            "binding-keyboard",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "binding-axis-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            480,
            720,
            1.0,
        ),
        (
            "binding-capture",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            800,
            600,
            1.0,
        ),
        (
            "autofire-game",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "autofire-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            480,
            1100,
            1.0,
        ),
        (
            "calibration-active",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            1100,
            1100,
            1.0,
        ),
        (
            "timing-active",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            1100,
            1500,
            1.0,
        ),
        (
            "audio-fallback",
            SettingsCategory::Audio,
            InputDevicesPage::Controls,
            800,
            900,
            1.0,
        ),
        (
            "storage-import",
            SettingsCategory::Storage,
            InputDevicesPage::Controls,
            480,
            900,
            1.0,
        ),
        (
            "main-menu-800x600",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            800,
            600,
            1.0,
        ),
        (
            "controls-1280x720",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1280,
            720,
            1.0,
        ),
        (
            "controls-1024x768",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1024,
            768,
            1.0,
        ),
        (
            "controls-800x600",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            800,
            600,
            1.0,
        ),
        (
            "controls-ws-1280x720",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1280,
            720,
            1.0,
        ),
        (
            "controls-light",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1100,
            1.0,
        ),
        (
            "controls-high-contrast",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1100,
            1.0,
        ),
        (
            "controls-retro",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1100,
            1.0,
        ),
        (
            "controls-system",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1300,
            1100,
            1.0,
        ),
        (
            "controls-game-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            480,
            1200,
            1.0,
        ),
        (
            "profiles-manage",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1100,
            1.0,
        ),
        (
            "search-conditional",
            SettingsCategory::Audio,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "search-no-results",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            480,
            900,
            1.0,
        ),
        (
            "input-tilt-scoped",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            1100,
            1000,
            1.0,
        ),
        (
            "controls-comfortable",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1050,
            1.0,
        ),
        (
            "general-desktop",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "audio-desktop",
            SettingsCategory::Audio,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "video-desktop",
            SettingsCategory::Video,
            InputDevicesPage::Controls,
            1100,
            1150,
            1.0,
        ),
        (
            "emulation-desktop",
            SettingsCategory::Emulation,
            InputDevicesPage::Controls,
            1100,
            1250,
            1.0,
        ),
        (
            "firmware-before-scan",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "firmware-after-scan",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            1180,
            1.0,
        ),
        (
            "firmware-details",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            1250,
            1.0,
        ),
        (
            "firmware-light",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            1180,
            1.0,
        ),
        (
            "firmware-scanning",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "firmware-narrow",
            SettingsCategory::Firmware,
            InputDevicesPage::Controls,
            480,
            1200,
            1.0,
        ),
        (
            "storage-desktop",
            SettingsCategory::Storage,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "camera-desktop",
            SettingsCategory::Camera,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "interface-dark",
            SettingsCategory::Interface,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "interface-light",
            SettingsCategory::Interface,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "debugger-desktop",
            SettingsCategory::Debugger,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "debugger-colors",
            SettingsCategory::Debugger,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "search-rewind",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            1100,
            900,
            1.0,
        ),
        (
            "search-colors",
            SettingsCategory::General,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "controls-standard",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-gb",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-gba",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-nes",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-pce2",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-pce6",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-ms",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-gg",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-sg1000",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-coleco",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-ws",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            960,
            1.0,
        ),
        (
            "controls-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            480,
            1100,
            1.0,
        ),
        (
            "controls-200pct",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1800,
            2.0,
        ),
        (
            "profiles-empty",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1000,
            1.0,
        ),
        (
            "profiles-saved",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            1100,
            1050,
            1.0,
        ),
        (
            "profiles-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::Controls,
            480,
            1500,
            1.0,
        ),
        (
            "hotkeys-desktop",
            SettingsCategory::InputDevices,
            InputDevicesPage::Hotkeys,
            1100,
            1100,
            1.0,
        ),
        (
            "test-desktop",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            1100,
            1000,
            1.0,
        ),
        (
            "test-narrow",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            480,
            1100,
            1.0,
        ),
        (
            "test-no-device",
            SettingsCategory::InputDevices,
            InputDevicesPage::TestAndCalibrate,
            1100,
            900,
            1.0,
        ),
        (
            "audio-narrow",
            SettingsCategory::Audio,
            InputDevicesPage::Controls,
            480,
            1100,
            1.0,
        ),
        (
            "video-narrow",
            SettingsCategory::Video,
            InputDevicesPage::Controls,
            480,
            1400,
            1.0,
        ),
    ] {
        let mut settings = Settings::default();
        if name != "profiles-empty" {
            settings
                .save_input_profile("Living room controllers")
                .map_err(anyhow::Error::msg)?;
        }
        let mut state = DebugWindowState::new();
        state.settings_ui.category = category;
        state.settings_ui.input_page = input_page;
        state.settings_ui.selected_profile_id = Some("profile-1".into());
        use controls::controller_diagram::DiagramKind;
        state.settings_ui.controller_layout = Some(match name {
            "controls-standard" | "controls-1280x720" | "controls-1024x768"
            | "controls-800x600" => DiagramKind::StandardGamepad,
            "controls-gb" => DiagramKind::GameBoy,
            "controls-nes" => DiagramKind::Nes,
            "controls-pce2" => DiagramKind::PceTwoButton,
            "controls-pce6" => DiagramKind::PceSixButton,
            "controls-ms" => DiagramKind::SegaMasterSystem,
            "controls-gg" => DiagramKind::GameGear,
            "controls-sg1000" => DiagramKind::Sg1000,
            "controls-ws" | "controls-ws-1280x720" => DiagramKind::WonderSwan,
            "controls-coleco" => DiagramKind::Coleco,
            _ => DiagramKind::GameBoyAdvance,
        });
        if matches!(
            name,
            "controls-system" | "controls-game-narrow" | "input-tilt-scoped"
        ) {
            use crate::settings::{
                BindingAction, BindingTarget, GameplayBindingSource, InputGameKey, InputScope,
                InputSystem, PhysicalBinding,
            };
            let system = InputSystem::GameBoyAdvance;
            let game = InputGameKey::new(system, [0x42; 32]);
            state.settings_ui.current_input_game = Some(game.clone());
            state.settings_ui.current_input_game_name = Some("Example Adventure".into());
            state.settings_ui.input_scope = if name == "controls-game-narrow" {
                InputScope::Game(game)
            } else {
                InputScope::System(system)
            };
            settings
                .set_binding(
                    &state.settings_ui.input_scope,
                    BindingTarget::Joypad {
                        player: 1,
                        action: BindingAction::A,
                    },
                    GameplayBindingSource::Gamepad,
                    Some(PhysicalBinding::Gamepad("North".into())),
                )
                .map_err(anyhow::Error::msg)?;
            settings
                .set_binding(
                    &state.settings_ui.input_scope,
                    BindingTarget::Joypad {
                        player: 1,
                        action: BindingAction::B,
                    },
                    GameplayBindingSource::Gamepad,
                    None,
                )
                .map_err(anyhow::Error::msg)?;
        }
        if name == "controls-comfortable" {
            settings.ui.ui_density = crate::settings::UiDensity::Comfortable;
        }
        if name == "profiles-manage" {
            state.settings_ui.profile_manage_open = true;
        }
        if name == "search-no-results" {
            state.settings_ui.search = "xyzzy nonexistent".into();
        }
        if name == "search-conditional" {
            settings.audio.low_pass_enabled = false;
        }
        if name == "controls-gb" {
            state.settings_ui.binding_source = controls::BindingSource::Keyboard;
        }
        if matches!(
            name,
            "interface-light" | "firmware-light" | "controls-light"
        ) {
            settings.ui.theme_preset = crate::settings::UiThemePreset::Light;
        }
        if name == "controls-high-contrast" {
            settings.ui.theme_preset = crate::settings::UiThemePreset::HighContrastDark;
        }
        if name == "controls-retro" {
            settings.ui.theme_preset = crate::settings::UiThemePreset::Retro;
        }
        state.camera_devices_needs_refresh = false;
        if name == "search-rewind" {
            state.settings_ui.search = "rewind".into();
        }
        if name == "search-colors" {
            state.settings_ui.search = "color".into();
        }
        if name.starts_with("firmware-")
            && name != "firmware-before-scan"
            && name != "firmware-scanning"
        {
            state.firmware_inventory.needs_refresh = false;
            state.firmware_inventory.rows = firmware_rows();
        }
        let mut _scan_sender = None;
        if name == "firmware-scanning" {
            let (sender, receiver) = std::sync::mpsc::channel();
            _scan_sender = Some(sender);
            state.firmware_inventory.scan_receiver = Some(receiver);
        }
        let fingerprint = crate::settings::GamepadFingerprint {
            name: "Wireless Controller".into(),
            uuid: "030000005e0400008e02000000000000".into(),
        };
        for index in 0..2 {
            state
                .settings_ui
                .gamepad_snapshot
                .devices
                .push(crate::input::GamepadDeviceSnapshot {
                    id: crate::input::RuntimeGamepadId(index + 1),
                    fingerprint: fingerprint.clone(),
                    buttons: if index == 0 {
                        vec!["South".into()]
                    } else {
                        Vec::new()
                    },
                    left_stick: if index == 0 {
                        (0.72, -0.15)
                    } else {
                        (0.02, 0.01)
                    },
                    right_stick: if index == 0 {
                        (-0.42, 0.65)
                    } else {
                        (0.0, 0.0)
                    },
                    calibrated_left_stick: if index == 0 {
                        (0.72, -0.15)
                    } else {
                        (0.02, 0.01)
                    },
                    calibrated_right_stick: if index == 0 {
                        (-0.42, 0.65)
                    } else {
                        (0.0, 0.0)
                    },
                    waiting_for_neutral: false,
                });
            let player = &mut state.settings_ui.gamepad_snapshot.players[index as usize];
            player.status = crate::input::GamepadAssignmentStatus::Connected;
            player.device = Some(crate::input::RuntimeGamepadId(index + 1));
            player.buttons = if index == 0 {
                crate::input::HostButton::A.host_mask_bit()
                    | u16::from(crate::input::transforms::stick_dpad_mask(
                        (0.72, -0.15),
                        settings.tilt.deadzone,
                    ))
            } else {
                0
            };
        }
        if name == "test-no-device" {
            state.settings_ui.gamepad_snapshot = crate::input::GamepadSnapshot::default();
        }
        configure_roadmap_fixture(name, &mut settings, &mut state)?;
        render(
            &device,
            &queue,
            &output.join(format!("{name}.png")),
            width,
            height,
            scale,
            &mut settings,
            &mut state,
        )?;
    }
    Ok(())
}

fn configure_roadmap_fixture(
    name: &str,
    settings: &mut Settings,
    state: &mut DebugWindowState,
) -> anyhow::Result<()> {
    use crate::settings::{
        AutofireOverride, AutofirePattern, AutofireTarget, AxisBinding, AxisDirection,
        BindingAction, BindingExpression, BindingSet, BindingTarget, GameplayBindingSource,
        InputAxis, InputGameKey, InputScope, InputSystem,
    };
    if name.starts_with("binding-") {
        let target = BindingTarget::Joypad {
            player: 1,
            action: BindingAction::A,
        };
        let source = if name == "binding-axis-narrow" {
            GameplayBindingSource::Gamepad
        } else {
            state.settings_ui.binding_source = controls::BindingSource::Keyboard;
            GameplayBindingSource::Keyboard
        };
        let expression = if source == GameplayBindingSource::Keyboard {
            BindingExpression::chord(vec![
                BindingExpression::keyboard(winit::keyboard::KeyCode::ControlRight),
                BindingExpression::keyboard(winit::keyboard::KeyCode::KeyJ),
            ])
        } else {
            BindingExpression::axis(AxisBinding::new(InputAxis::RightX, AxisDirection::Positive))
        };
        let mut set = BindingSet::new(expression);
        set.add_expression(if source == GameplayBindingSource::Keyboard {
            BindingExpression::keyboard(winit::keyboard::KeyCode::KeyK)
        } else {
            BindingExpression::gamepad_button("North")
        })
        .map_err(anyhow::Error::msg)?;
        settings
            .set_binding_set(&InputScope::Global, target, source, Some(set))
            .map_err(anyhow::Error::msg)?;
        state.settings_ui.last_controller_layout = state.settings_ui.controller_layout;
        state.settings_ui.binding_editor = Some(controls::binding_editor::BindingEditor::open(
            InputScope::Global,
            target,
            source,
            "P1 A".into(),
            settings.binding_set(&InputScope::Global, target, source),
        ));
    }
    if name.starts_with("autofire-") {
        let game = InputGameKey::new(InputSystem::GameBoyAdvance, [0x42; 32]);
        state.settings_ui.current_input_game = Some(game.clone());
        state.settings_ui.current_input_game_name = Some("Example Adventure".into());
        state.settings_ui.input_scope = InputScope::Game(game);
        settings
            .set_autofire(
                &InputScope::Global,
                AutofireTarget {
                    player: 1,
                    action: BindingAction::B,
                },
                AutofireOverride::enabled(AutofirePattern::default()),
            )
            .map_err(anyhow::Error::msg)?;
        settings
            .set_autofire(
                &state.settings_ui.input_scope,
                AutofireTarget {
                    player: 1,
                    action: BindingAction::A,
                },
                AutofireOverride::enabled(AutofirePattern {
                    period_frames: 6,
                    on_frames: 2,
                }),
            )
            .map_err(anyhow::Error::msg)?;
    }
    if name == "audio-fallback" {
        settings.audio.output_device_id = Some("example-disconnected-device".into());
        settings.audio.buffer_policy = crate::settings::AudioBufferPolicy::LowLatency;
        state.settings_ui.audio_host_status = crate::audio::AudioHostStatus {
            active_device: Some(crate::audio::AudioOutputDevice {
                id: "example-default".into(),
                name: "Speakers (default output)".into(),
            }),
            device_fallback: Some(
                "The saved output is unavailable. Using the default output.".into(),
            ),
            buffer_fallback: Some(crate::audio::AudioBufferFallback {
                requested: crate::settings::AudioBufferPolicy::LowLatency,
                active: crate::settings::AudioBufferPolicy::Auto,
                underrun_reports: 3,
            }),
        };
    }
    if name == "storage-import" {
        state.settings_ui.settings_import_json = Some(settings.export_settings_json()?);
        state.settings_ui.settings_file_notice = Some("Settings file ready to import.".into());
    }
    if name == "timing-active" {
        use std::time::Duration;
        let timing = &mut state.settings_ui.input_timing;
        timing.set_enabled(true);
        let start = crate::platform::Instant::now();
        for index in 0..32 {
            let event = start + Duration::from_millis(index * 8);
            timing.observe_poll(crate::input::timing::InputPollTiming {
                latest_event: Some(event),
                observed_events: 1,
                snapshot_complete: event + Duration::from_micros(700),
            });
            timing.frame_reached(event + Duration::from_millis(2));
            timing.frame_submitted(event + Duration::from_millis(3));
        }
    }
    Ok(())
}
