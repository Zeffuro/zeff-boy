use crate::debug::DebugWindowState;
use crate::hardware::types::hardware_mode::HardwareModePreference;
use crate::settings::{
    BindingAction, InputBindingAction, LeftStickMode, Settings, ShortcutAction, TiltBindingAction,
    TiltInputMode,
};

pub(crate) fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut Settings,
    state: &mut DebugWindowState,
    open: &mut bool,
) {
    egui::Window::new("Settings")
        .open(open)
        .default_width(400.0)
        .show(ctx, |ui| {
            const TABS: &[&str] = &["Emulation", "Controls", "Audio", "UI"];

            ui.horizontal(|ui| {
                for (i, &label) in TABS.iter().enumerate() {
                    if ui
                        .selectable_label(state.settings_tab == i, label)
                        .clicked()
                    {
                        state.settings_tab = i;
                    }
                }
            });
            ui.separator();

            match state.settings_tab {
                0 => draw_settings_emulation(ui, settings),
                1 => draw_settings_controls(ui, settings, state),
                2 => draw_settings_audio(ui, settings),
                3 => draw_settings_ui(ui, settings),
                _ => {}
            }

            ui.separator();
            if ui.button("Reset to defaults").clicked() {
                *settings = Settings::default();
                state.rebinding_action = None;
            }
        });
}

fn draw_settings_emulation(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Hardware");
    egui::ComboBox::from_label("Hardware mode")
        .selected_text(match settings.hardware_mode_preference {
            HardwareModePreference::Auto => "Auto",
            HardwareModePreference::ForceDmg => "DMG",
            HardwareModePreference::ForceCgb => "CGB",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut settings.hardware_mode_preference,
                HardwareModePreference::Auto,
                "Auto",
            );
            ui.selectable_value(
                &mut settings.hardware_mode_preference,
                HardwareModePreference::ForceDmg,
                "DMG",
            );
            ui.selectable_value(
                &mut settings.hardware_mode_preference,
                HardwareModePreference::ForceCgb,
                "CGB",
            );
        });

    ui.separator();
    ui.heading("Speed");
    ui.add(
        egui::Slider::new(&mut settings.fast_forward_multiplier, 1..=16)
            .text("Fast-forward multiplier"),
    );
    ui.add(
        egui::Slider::new(&mut settings.uncapped_frames_per_tick, 1..=240)
            .text("Uncapped frames/tick"),
    );
    ui.checkbox(&mut settings.uncapped_speed, "Start in uncapped mode");
    ui.checkbox(&mut settings.frame_skip, "Frame skip when behind")
        .on_hover_text(
            "When enabled, skip emulation frames to stay in real-time if the \
             host can't keep up. When disabled, the emulator catches up \
             gradually (more accurate, may drift behind).",
        );
    ui.checkbox(&mut settings.auto_save_state, "Auto save/load state")
        .on_hover_text(
            "Automatically save emulator state when closing and \
             restore it when loading the same ROM.",
        );

    ui.separator();
    ui.heading("Rewind");
    ui.checkbox(&mut settings.rewind_enabled, "Enable rewind")
        .on_hover_text(
            "Hold the rewind key to rewind gameplay. \
             Captures a snapshot every 4 frames (~15 fps capture rate).",
        );
    ui.horizontal(|ui| {
        ui.label("History (seconds):");
        ui.add(
            egui::DragValue::new(&mut settings.rewind_seconds)
                .range(1..=120)
                .speed(1),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Rewind speed:");
        ui.add(
            egui::DragValue::new(&mut settings.rewind_speed)
                .range(1..=10)
                .speed(1),
        );
        ui.label(match settings.rewind_speed {
            1 => "(fastest — pop every tick)",
            2 => "(fast)",
            3..=4 => "(normal)",
            _ => "(slow)",
        });
    });
}

fn draw_settings_controls(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut DebugWindowState,
) {
    ui.heading("System Shortcuts");
    if state.rebinding_shortcut.is_some() {
        ui.label(egui::RichText::new("Press a key to rebind...").color(egui::Color32::YELLOW));
    }
    egui::Grid::new("system_shortcuts")
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Speed-up (hold)");
            let key_label = if state.rebinding_speedup {
                format!("Press key... ({})", settings.speedup_key)
            } else {
                settings.speedup_key.clone()
            };
            if ui.button(key_label).clicked() {
                state.rebinding_speedup = true;
                state.rebinding_action = None;
                state.rebinding_shortcut = None;
                state.rebinding_rewind = false;
            }
            ui.end_row();

            ui.label("Rewind (hold)");
            let rewind_label = if state.rebinding_rewind {
                format!("Press key... ({})", settings.rewind_key)
            } else {
                settings.rewind_key.clone()
            };
            if ui.button(rewind_label).clicked() {
                state.rebinding_rewind = true;
                state.rebinding_action = None;
                state.rebinding_shortcut = None;
                state.rebinding_speedup = false;
            }
            ui.end_row();

            for &action in ShortcutAction::ALL {
                ui.label(action.label());
                let key_str = settings.shortcut_bindings.key_str(action).to_owned();
                let capture_label = if state.rebinding_shortcut == Some(action) {
                    format!("Press key... ({key_str})")
                } else {
                    key_str
                };
                if ui.button(capture_label).clicked() {
                    state.rebinding_shortcut = Some(action);
                    state.rebinding_action = None;
                    state.rebinding_speedup = false;
                    state.rebinding_rewind = false;
                }
                ui.end_row();
            }
        });

    ui.separator();
    ui.heading("Joypad Bindings");
    if let Some(action) = state.rebinding_action {
        let label = match action {
            InputBindingAction::Joypad(a) => joypad_binding_label(a),
            InputBindingAction::Tilt(a) => tilt_binding_label(a),
        };
        ui.label(
            egui::RichText::new(format!("Press a key for {}...", label))
                .color(egui::Color32::YELLOW),
        );
    }
    egui::Grid::new("joypad_bindings")
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for action in [
                BindingAction::Up,
                BindingAction::Down,
                BindingAction::Left,
                BindingAction::Right,
                BindingAction::A,
                BindingAction::B,
                BindingAction::Start,
                BindingAction::Select,
            ] {
                ui.label(joypad_binding_label(action));
                let key_name = format!("{:?}", settings.key_bindings.get(action));
                let capture_label =
                    if state.rebinding_action == Some(InputBindingAction::Joypad(action)) {
                        format!("Press key... ({key_name})")
                    } else {
                        key_name
                    };
                if ui.button(capture_label).clicked() {
                    state.rebinding_action = Some(InputBindingAction::Joypad(action));
                }
                ui.end_row();
            }
        });

    ui.separator();
    ui.heading("Gamepad Bindings");
    if state.rebinding_gamepad.is_some() {
        ui.label(egui::RichText::new("Press a gamepad button...").color(egui::Color32::YELLOW));
    }
    egui::Grid::new("gamepad_bindings")
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for action in [
                BindingAction::Up,
                BindingAction::Down,
                BindingAction::Left,
                BindingAction::Right,
                BindingAction::A,
                BindingAction::B,
                BindingAction::Start,
                BindingAction::Select,
            ] {
                ui.label(joypad_binding_label(action));
                let button_name = settings.gamepad_bindings.get(action).to_owned();
                let capture_label = if state.rebinding_gamepad == Some(action) {
                    format!("Press button... ({button_name})")
                } else {
                    button_name
                };
                if ui.button(capture_label).clicked() {
                    state.rebinding_gamepad = Some(action);
                    state.rebinding_action = None;
                    state.rebinding_shortcut = None;
                    state.rebinding_speedup = false;
                    state.rebinding_rewind = false;
                }
                ui.end_row();
            }
        });
    if ui.button("Reset gamepad to defaults").clicked() {
        settings.gamepad_bindings = crate::settings::GamepadBindings::default();
        state.rebinding_gamepad = None;
    }

    ui.separator();
    ui.heading("MBC7 Tilt");
    egui::ComboBox::from_label("Left stick behavior")
        .selected_text(match settings.left_stick_mode {
            LeftStickMode::Auto => "Auto (Tilt on MBC7, D-pad otherwise)",
            LeftStickMode::Tilt => "Always Tilt",
            LeftStickMode::Dpad => "Always D-pad",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut settings.left_stick_mode,
                LeftStickMode::Auto,
                "Auto (Tilt on MBC7, D-pad otherwise)",
            );
            ui.selectable_value(
                &mut settings.left_stick_mode,
                LeftStickMode::Tilt,
                "Always Tilt",
            );
            ui.selectable_value(
                &mut settings.left_stick_mode,
                LeftStickMode::Dpad,
                "Always D-pad",
            );
        });
    egui::ComboBox::from_label("Tilt input source")
        .selected_text(match settings.tilt_input_mode {
            TiltInputMode::Keyboard => "Keyboard (WASD)",
            TiltInputMode::Mouse => "Mouse",
            TiltInputMode::Auto => "Auto-detect",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut settings.tilt_input_mode,
                TiltInputMode::Keyboard,
                "Keyboard (WASD)",
            );
            ui.selectable_value(&mut settings.tilt_input_mode, TiltInputMode::Mouse, "Mouse");
            ui.selectable_value(
                &mut settings.tilt_input_mode,
                TiltInputMode::Auto,
                "Auto-detect",
            );
        });
    ui.checkbox(&mut settings.tilt_invert_x, "Invert tilt X");
    ui.checkbox(&mut settings.tilt_invert_y, "Invert tilt Y");
    ui.checkbox(
        &mut settings.stick_tilt_bypass_lerp,
        "Direct left-stick tilt (bypass lerp)",
    );
    ui.add(egui::Slider::new(&mut settings.tilt_sensitivity, 0.1..=3.0).text("Tilt sensitivity"));
    ui.add(egui::Slider::new(&mut settings.tilt_lerp, 0.0..=1.0).text("Tilt smoothing"));
    ui.add(egui::Slider::new(&mut settings.tilt_deadzone, 0.0..=0.5).text("Tilt deadzone"));

    ui.separator();
    ui.heading("Tilt Key Bindings");
    if ui.button("Reset tilt keys to WASD").clicked() {
        settings.tilt_key_bindings.set_wasd_defaults();
    }
    egui::Grid::new("tilt_bindings")
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for action in [
                TiltBindingAction::Up,
                TiltBindingAction::Down,
                TiltBindingAction::Left,
                TiltBindingAction::Right,
            ] {
                ui.label(tilt_binding_label(action));
                let key_name = format!("{:?}", settings.tilt_key_bindings.get(action));
                let capture_label =
                    if state.rebinding_action == Some(InputBindingAction::Tilt(action)) {
                        format!("Press key... ({key_name})")
                    } else {
                        key_name
                    };
                if ui.button(capture_label).clicked() {
                    state.rebinding_action = Some(InputBindingAction::Tilt(action));
                }
                ui.end_row();
            }
        });
}

fn draw_settings_audio(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Volume");
    ui.add(
        egui::Slider::new(&mut settings.master_volume, 0.0..=1.0)
            .text("Master volume")
            .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
    );
    ui.checkbox(
        &mut settings.mute_audio_during_fast_forward,
        "Mute audio while fast-forward is held",
    );

    ui.separator();
    ui.heading("Recording");

    use crate::settings::AudioRecordingFormat;
    egui::ComboBox::from_label("Recording format")
        .selected_text(settings.audio_recording_format.label())
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::Wav16,
                AudioRecordingFormat::Wav16.label(),
            );
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::WavFloat,
                AudioRecordingFormat::WavFloat.label(),
            );
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::OggVorbis,
                AudioRecordingFormat::OggVorbis.label(),
            );
            ui.selectable_value(
                &mut settings.audio_recording_format,
                AudioRecordingFormat::Midi,
                AudioRecordingFormat::Midi.label(),
            );
        });
    ui.label(
        egui::RichText::new(
            "16-bit PCM: smaller files, standard compatibility.\n\
             32-bit Float: lossless sample precision, ideal for editing.\n\
             OGG Vorbis: compressed lossy format, much smaller files.\n\
             MIDI: records APU channel notes/volumes as a Standard MIDI File.",
        )
        .weak()
        .small(),
    );
}

fn draw_settings_ui(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Display");
    ui.checkbox(&mut settings.show_fps, "Show FPS in debug panel");
    ui.checkbox(&mut settings.enable_memory_editing, "Enable memory editing")
        .on_hover_text("Allow writing to memory addresses in the Memory Viewer");
    ui.checkbox(&mut settings.autohide_menu_bar, "Autohide menu bar")
        .on_hover_text(
            "Hide the menu bar when the cursor moves away from the top edge. \
             Hover near the top to reveal it.",
        );

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
        });
    ui.label(
        egui::RichText::new(
            "None: raw RGB555 colors expanded to 8-bit per channel.\n\
             GBC LCD: simulates the color response of the Game Boy Color LCD panel,\n\
             which shifts colors toward a warmer, slightly washed-out appearance.",
        )
        .weak()
        .small(),
    );
}

fn joypad_binding_label(action: BindingAction) -> &'static str {
    match action {
        BindingAction::Up => "Up",
        BindingAction::Down => "Down",
        BindingAction::Left => "Left",
        BindingAction::Right => "Right",
        BindingAction::A => "A",
        BindingAction::B => "B",
        BindingAction::Start => "Start",
        BindingAction::Select => "Select",
    }
}

fn tilt_binding_label(action: TiltBindingAction) -> &'static str {
    match action {
        TiltBindingAction::Up => "Tilt Up",
        TiltBindingAction::Down => "Tilt Down",
        TiltBindingAction::Left => "Tilt Left",
        TiltBindingAction::Right => "Tilt Right",
    }
}
