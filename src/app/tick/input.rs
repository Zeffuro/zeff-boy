use super::super::App;
use super::super::PendingReplayBatch;
use crate::app::keyboard::HeldFrontendAction;
use crate::emu_thread::PceMouseInput;
use crate::emu_thread::ReplayJoypadFrame;
use crate::settings::GamepadAction;

fn encode_pce_mouse_axis(delta: f64, sensitivity: f32) -> i16 {
    (-delta * f64::from(sensitivity.clamp(0.25, 4.0)))
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

impl App {
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn cancel_hidden_settings_capture(&mut self) {
        if self
            .gfx
            .as_ref()
            .and_then(crate::graphics::Graphics::settings_window)
            .is_some_and(|window| {
                window.is_minimized() == Some(true) || window.is_visible() == Some(false)
            })
        {
            self.clear_rebinding_state();
        }
    }

    pub(super) fn pce_mouse_input(&mut self, consume_motion: bool) -> PceMouseInput {
        let mode = self.settings.emulation.pce_controller.core_mode();
        let memory_base_mode = self.settings.emulation.pce_memory_base.core_mode();
        if self.active_system != crate::emu_backend::ActiveSystem::Pce {
            self.pce_mouse_motion = (0.0, 0.0);
            return PceMouseInput {
                mode,
                memory_base_mode,
                ..Default::default()
            };
        }

        let motion = if consume_motion {
            std::mem::replace(&mut self.pce_mouse_motion, (0.0, 0.0))
        } else {
            (0.0, 0.0)
        };
        let sensitivity = self.settings.emulation.pce_mouse_sensitivity;
        let buttons = u8::from(self.mouse_left_pressed) | (u8::from(self.mouse_right_pressed) << 1);
        PceMouseInput {
            mode,
            memory_base_mode,
            delta_x: encode_pce_mouse_axis(motion.0, sensitivity),
            delta_y: encode_pce_mouse_axis(motion.1, sensitivity),
            buttons,
        }
    }

    pub(in crate::app) fn poll_gamepad(&mut self) {
        self.sync_effective_input();
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_hidden_settings_capture();
        let supports_rewind = self.core_supports_rewind();
        let capture_active = self.debug_windows.settings_ui.captures_gameplay_input()
            || self.debug_windows.rebinding_gamepad.is_some()
            || self.debug_windows.rebinding_gamepad_p2.is_some()
            || self.debug_windows.rebinding_gamepad_pce_multitap.is_some()
            || self.debug_windows.rebinding_ws_gamepad.is_some()
            || self.debug_windows.rebinding_gamepad_action.is_some()
            || self.debug_windows.rebinding_action.is_some()
            || self.debug_windows.rebinding_shortcut.is_some()
            || self.debug_windows.rebinding_speedup
            || self.debug_windows.rebinding_rewind;
        self.sync_keyboard_capture(capture_active);
        #[cfg(not(target_arch = "wasm32"))]
        let timing_visible = self.live_input_settings_visible();
        #[cfg(target_arch = "wasm32")]
        let timing_visible = self.show_settings_window && self.wasm_tab_visible.get();
        let measure_timing = timing_visible && self.debug_windows.settings_ui.wants_input_timing();
        if !measure_timing {
            self.debug_windows.settings_ui.input_timing.pause();
        }
        let Some(gamepad) = &mut self.gamepad else {
            self.debug_windows.settings_ui.gamepad_commands.clear();
            self.debug_windows.settings_ui.gamepad_snapshot = Default::default();
            return;
        };
        for command in self.debug_windows.settings_ui.gamepad_commands.drain(..) {
            gamepad.apply_command(command);
        }
        let poll = gamepad.poll(
            &self.input_configuration.resolved,
            &self.settings.input_devices,
            capture_active,
            self.input_configuration.resolved.tilt.deadzone,
            measure_timing,
        );
        if let Some(timing) = poll.timing {
            self.debug_windows
                .settings_ui
                .input_timing
                .observe_poll(timing);
        }
        let mut snapshot = gamepad.snapshot();
        let implicit_stick_enabled = self.input_configuration.resolved.tilt.left_stick_mode
            != crate::settings::LeftStickMode::BindingsOnly;
        for (index, (player, stick)) in snapshot
            .players
            .iter_mut()
            .zip(poll.player_sticks)
            .enumerate()
        {
            if implicit_stick_enabled
                && (index != 0
                    || self.left_stick_controls_dpad(
                        self.rom_info.is_mbc7 || self.rom_info.is_gba_tilt,
                    ))
            {
                player.buttons |= u16::from(crate::input::transforms::stick_dpad_mask(
                    stick,
                    self.input_configuration.resolved.tilt.deadzone,
                ));
            }
        }
        let capture_player = if self.debug_windows.rebinding_gamepad_p2.is_some() {
            2
        } else {
            self.debug_windows
                .rebinding_gamepad_pce_multitap
                .map_or(1, |(player, _)| player)
        };
        let capture_device = snapshot
            .players
            .get(usize::from(capture_player - 1))
            .and_then(|player| player.device);
        let captured = poll
            .raw_pressed
            .iter()
            .find(|press| capture_device.is_none_or(|id| id == press.device))
            .map(|press| press.button);
        self.debug_windows.settings_ui.gamepad_snapshot = snapshot;

        // Always deliver reducer transitions: entering capture/disconnecting must release old input.
        for (key, pressed) in poll.events {
            self.host_input.set_gamepad(key, pressed);
        }
        for (key, pressed) in poll.events_p2 {
            self.host_input.set_gamepad_p2(key, pressed);
        }
        for (key, pressed) in poll.events_p3 {
            self.host_input.set_gamepad_p3(key, pressed);
        }
        for (key, pressed) in poll.events_p4 {
            self.host_input.set_gamepad_p4(key, pressed);
        }
        for (key, pressed) in poll.events_p5 {
            self.host_input.set_gamepad_p5(key, pressed);
        }
        for (button, pressed) in poll.ws_events {
            self.host_input.set_ws_gamepad(button, pressed);
        }
        for (action, pressed) in poll.action_events {
            match action {
                GamepadAction::SpeedUp => {
                    self.set_gamepad_frontend_hold(HeldFrontendAction::FastForward, pressed)
                }
                GamepadAction::Rewind => self.set_gamepad_frontend_hold(
                    HeldFrontendAction::Rewind,
                    supports_rewind && pressed,
                ),
                GamepadAction::Pause => {
                    if pressed {
                        self.toggle_user_paused();
                    }
                }
                GamepadAction::Turbo => {
                    self.set_gamepad_frontend_hold(HeldFrontendAction::Turbo, pressed)
                }
            }
        }
        self.tilt.left_stick = poll.left_stick;
        self.host_input.set_multiplayer_gamepad_sticks(
            if self.input_configuration.resolved.tilt.left_stick_mode
                == crate::settings::LeftStickMode::BindingsOnly
            {
                [(0.0, 0.0); 5]
            } else {
                poll.player_sticks
            },
            self.input_configuration.resolved.tilt.deadzone,
        );

        if let Some(button_name) = captured {
            use crate::settings::{BindingTarget, GameplayBindingSource, PhysicalBinding};
            let target = if let Some(action) = self.debug_windows.rebinding_gamepad {
                Some(BindingTarget::Joypad { player: 1, action })
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_p2 {
                Some(BindingTarget::Joypad { player: 2, action })
            } else if let Some((player, action)) = self.debug_windows.rebinding_gamepad_pce_multitap
            {
                Some(BindingTarget::Joypad { player, action })
            } else {
                self.debug_windows
                    .rebinding_ws_gamepad
                    .map(BindingTarget::WonderSwan)
            };
            if let Some(target) = target {
                self.commit_gameplay_binding(
                    target,
                    GameplayBindingSource::Gamepad,
                    PhysicalBinding::Gamepad(button_name.into()),
                );
                self.clear_rebinding_state();
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_action {
                self.settings
                    .gamepad_bindings
                    .set_action(action, button_name);
                self.debug_windows.rebinding_gamepad_action = None;
                self.settings.save();
            }
        }
        self.observe_autofire_host_transitions();
    }

    pub(super) fn current_replay_recordable_joypad_input(&mut self) -> (u8, u8) {
        self.current_host_joypad_input()
    }

    pub(in crate::app) fn observe_autofire_host_transitions(&mut self) {
        let buttons = self.current_host_button_masks();
        let configured = self.effective_autofire_patterns();
        let legacy_held = self.speed.turbo_held;
        if self.autofire_observed_legacy_held && !legacy_held {
            self.autofire_legacy_release_pending = true;
        }
        for (player, patterns) in configured.iter().enumerate() {
            for (action, pattern) in patterns.iter().enumerate() {
                if pattern.is_none() {
                    continue;
                }
                let bit = 1 << autofire_action_bit(action);
                if self.autofire_observed_buttons[player] & bit != 0 && buttons[player] & bit == 0 {
                    self.autofire_released_targets[player][action] = true;
                }
            }
        }
        self.autofire_observed_buttons = buttons;
        self.autofire_observed_legacy_held = legacy_held;
    }

    pub(super) fn autofire_requires_serialized_frame(&self) -> bool {
        crate::app::autofire::needs_staging(
            self.speed.turbo_held,
            self.effective_autofire_patterns(),
            self.current_host_button_masks(),
        )
    }

    pub(in crate::app) fn consume_autofire_rearm_if_idle(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        let tas_idle =
            self.pending_tas_autofire.is_none() && !self.tas_control.live_frame_in_flight();
        #[cfg(target_arch = "wasm32")]
        let tas_idle = true;
        let has_pending_rearm = self.autofire_rearm_pending
            || self.autofire_legacy_release_pending
            || self
                .autofire_released_targets
                .iter()
                .flatten()
                .any(|pending| *pending);
        if has_pending_rearm
            && self.frames_in_flight == 0
            && self.recording.pending_replay_batches.is_empty()
            && tas_idle
        {
            if self.autofire_rearm_pending {
                self.autofire_state = crate::app::autofire::AutofireState::default();
            } else {
                if self.autofire_legacy_release_pending {
                    self.autofire_state.legacy_phase = 0;
                }
                for player in 0..5 {
                    for action in 0..8 {
                        if self.autofire_released_targets[player][action] {
                            self.autofire_state.configured[player][action] = 0;
                        }
                    }
                }
            }
            self.autofire_rearm_pending = false;
            self.autofire_released_targets = [[false; 8]; 5];
            self.autofire_legacy_release_pending = false;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn preview_autofire_tas_input(
        &mut self,
        input: &mut crate::tas_project::TasInputFrame,
    ) -> Option<crate::app::autofire::AutofireState> {
        self.consume_autofire_rearm_if_idle();
        let coleco = self.active_system == crate::emu_backend::ActiveSystem::Coleco;
        let buttons = if coleco {
            let mut buttons = [0; 5];
            for (buttons, player) in buttons.iter_mut().zip(input.coleco) {
                *buttons = u8::from(player.left_button) | (u8::from(player.right_button) << 1);
            }
            buttons
        } else {
            input.players.map(|player| player.buttons)
        };
        let configured = self.effective_autofire_patterns();
        let legacy_held = self.speed.turbo_held;
        if !crate::app::autofire::needs_staging(legacy_held, configured, buttons) {
            crate::app::autofire::reset_released(
                &mut self.autofire_state,
                legacy_held,
                configured,
                buttons,
            );
            return None;
        }
        let preview =
            crate::app::autofire::preview(self.autofire_state, legacy_held, configured, &[buttons]);
        if coleco {
            for (player, buttons) in input.coleco.iter_mut().zip(preview.buttons[0]) {
                player.left_button = buttons & 1 != 0;
                player.right_button = buttons & 2 != 0;
            }
        } else {
            for (player, buttons) in input.players.iter_mut().zip(preview.buttons[0]) {
                player.buttons = buttons;
            }
        }
        preview.states.into_iter().next()
    }

    pub(super) fn prepare_replay_joypad_batch(
        &mut self,
        frames_to_step: usize,
        buttons: u8,
        dpad: u8,
        host_tilt: (f32, f32),
        host_camera_frame: Option<&[u8]>,
    ) -> Option<Vec<ReplayJoypadFrame>> {
        if frames_to_step == 0 {
            return None;
        }

        if let Some(batch) = self.recording.pending_replay_batches.front() {
            return Some(batch.frames.iter().take(frames_to_step).cloned().collect());
        }

        if let Some(player) = self.recording.replay_player.as_ref() {
            let frames_to_step = player.frames_until_next_event(frames_to_step);
            let frames = player
                .peek_joypad_frames(self.recording.queued_replay_playback_frames, frames_to_step);

            if frames.is_empty() {
                self.toast_manager.info("Replay finished");
                self.recording.replay_player = None;
                self.recording.queued_replay_playback_frames = 0;
                return None;
            }

            self.recording.queued_replay_playback_frames += frames.len();
            self.recording
                .pending_replay_batches
                .push_back(PendingReplayBatch {
                    frames: frames.clone(),
                    record: false,
                    playback: true,
                    autofire_states: Vec::new(),
                });
            return Some(frames);
        }

        self.consume_autofire_rearm_if_idle();

        let record = self.recording.should_stage_replay_recording_input();
        let legacy_held = self.speed.turbo_held;
        let configured = self.effective_autofire_patterns();
        let players = [
            (buttons, dpad),
            self.current_host_joypad_p2_input(),
            self.current_host_joypad_p3_input(),
            self.current_host_joypad_p4_input(),
            self.current_host_joypad_p5_input(),
        ];
        let button_masks = players.map(|(buttons, _)| buttons);
        let autofire = crate::app::autofire::needs_staging(legacy_held, configured, button_masks);
        if record || autofire {
            let legacy_reset_pending = self.autofire_state.legacy_phase != 0;
            let preview = crate::app::autofire::preview(
                self.autofire_state,
                legacy_held,
                configured,
                &vec![button_masks; frames_to_step],
            );
            let zapper = if self.active_system == crate::emu_backend::ActiveSystem::Nes {
                self.nes_zapper_input().into()
            } else {
                Default::default()
            };
            let host_tilt = if self.rom_info.is_mbc7 || self.rom_info.is_gba_tilt {
                host_tilt
            } else {
                (0.0, 0.0)
            };
            let camera_frame = self
                .rom_info
                .is_pocket_camera
                .then(|| host_camera_frame.map(<[u8]>::to_vec))
                .flatten();
            let frames = preview
                .buttons
                .into_iter()
                .map(|pulse| ReplayJoypadFrame {
                    buttons: pulse[0],
                    dpad: players[0].1,
                    buttons_p2: pulse[1],
                    dpad_p2: players[1].1,
                    buttons_p3: pulse[2],
                    dpad_p3: players[2].1,
                    buttons_p4: pulse[3],
                    dpad_p4: players[3].1,
                    buttons_p5: pulse[4],
                    dpad_p5: players[4].1,
                    zapper,
                    host_tilt,
                    camera_frame: camera_frame.clone(),
                    coleco: Default::default(),
                })
                .collect::<Vec<_>>();
            self.recording
                .pending_replay_batches
                .push_back(PendingReplayBatch {
                    frames: frames.clone(),
                    record,
                    playback: false,
                    autofire_states: if autofire
                        || has_configured_autofire(configured)
                        || legacy_reset_pending
                    {
                        preview.states
                    } else {
                        Vec::new()
                    },
                });
            return Some(frames);
        }

        crate::app::autofire::reset_released(
            &mut self.autofire_state,
            legacy_held,
            configured,
            button_masks,
        );

        None
    }

    fn current_host_button_masks(&self) -> [u8; 5] {
        [
            self.current_host_joypad_input().0,
            self.current_host_joypad_p2_input().0,
            self.current_host_joypad_p3_input().0,
            self.current_host_joypad_p4_input().0,
            self.current_host_joypad_p5_input().0,
        ]
    }

    fn effective_autofire_patterns(&self) -> [[Option<crate::settings::AutofirePattern>; 8]; 5] {
        let mut configured = self.input_configuration.autofire;
        if self.active_system == crate::emu_backend::ActiveSystem::WonderSwan {
            for (player, patterns) in configured.iter_mut().enumerate() {
                for (action, pattern) in patterns.iter_mut().enumerate() {
                    if player != 0 || !matches!(action, 0 | 1 | 6) {
                        *pattern = None;
                    }
                }
            }
        }
        configured
    }
}

const fn autofire_action_bit(action: usize) -> u8 {
    match action {
        0 => 0,
        1 => 1,
        2 => 6,
        3 => 7,
        4 => 4,
        5 => 5,
        6 => 2,
        7 => 3,
        _ => unreachable!(),
    }
}

fn has_configured_autofire(configured: [[Option<crate::settings::AutofirePattern>; 8]; 5]) -> bool {
    configured.iter().flatten().any(Option::is_some)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::tas_control::tests::harness::app_with_worker;
    use crate::emu_backend::{EmuBackend, PceBackend};
    #[cfg(not(target_arch = "wasm32"))]
    use crate::emu_thread::EmuResponsePoll;
    use crate::emu_thread::{EmuThread, FrameResult, ReplayJoypadFrame};
    use crate::input::HostButton;
    use crate::settings::{
        AutofireOverride, AutofirePattern, AutofireTarget, BindingAction, InputScope,
    };

    fn app_for_autofire() -> App {
        let path = std::path::PathBuf::from("autofire-test.pce");
        let mut rom = vec![0xEA; 0x2000];
        rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let backend = EmuBackend::from_pce(PceBackend::new(rom, path.clone()).unwrap());
        app_with_worker(
            EmuThread::spawn(backend, false),
            1,
            crate::emu_backend::ActiveSystem::Pce,
            path,
        )
    }

    #[test]
    fn pce_mouse_axis_uses_host_relative_sign_sensitivity_and_saturation() {
        assert_eq!(encode_pce_mouse_axis(8.0, 1.25), -10);
        assert_eq!(encode_pce_mouse_axis(-8.0, 1.25), 10);
        assert_eq!(encode_pce_mouse_axis(8.0, 0.0), -2);
        assert_eq!(encode_pce_mouse_axis(8.0, 99.0), -32);
        assert_eq!(encode_pce_mouse_axis(-1_000_000.0, 1.0), i16::MAX);
    }

    #[test]
    fn staged_autofire_keeps_player_masks_and_commits_only_executed_prefixes() {
        let mut app = app_for_autofire();
        let pattern = AutofirePattern {
            period_frames: 2,
            on_frames: 1,
        };
        for player in [1, 5] {
            app.settings
                .set_autofire(
                    &InputScope::Global,
                    AutofireTarget {
                        player,
                        action: BindingAction::A,
                    },
                    AutofireOverride::enabled(pattern),
                )
                .unwrap();
        }
        app.sync_effective_input();
        app.host_input.set_remote(HostButton::A, true);
        app.host_input.set_remote_p5(HostButton::A, true);
        let (buttons, dpad) = app.current_host_joypad_input();
        let frames = app
            .prepare_replay_joypad_batch(2, buttons, dpad, (0.0, 0.0), None)
            .unwrap();
        assert_eq!(frames[0].buttons, 1);
        assert_eq!(frames[0].buttons_p5, 1);
        assert_eq!(frames[1].buttons, 0);
        assert_eq!(frames[1].buttons_p5, 0);
        assert_eq!(
            app.autofire_state,
            crate::app::autofire::AutofireState::default()
        );
        assert_eq!(
            app.recording.pending_replay_batches[0]
                .autofire_states
                .len(),
            2
        );
        app.commit_replay_batch(0);
        assert_eq!(
            app.autofire_state,
            crate::app::autofire::AutofireState::default()
        );
        assert_eq!(app.recording.pending_replay_batches[0].frames.len(), 2);

        app.commit_replay_batch(1);
        assert_eq!(app.autofire_state.configured[0][0], 1);
        assert_eq!(app.autofire_state.configured[4][0], 1);
        assert_eq!(app.recording.pending_replay_batches[0].frames.len(), 1);
        assert_eq!(
            app.recording.pending_replay_batches[0]
                .autofire_states
                .len(),
            1
        );

        app.commit_replay_batch(1);
        assert_eq!(app.autofire_state.configured[0][0], 0);
        assert_eq!(app.autofire_state.configured[4][0], 0);
        assert!(app.recording.pending_replay_batches.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn replay_start_ack_drains_prior_staged_frame_before_recording() {
        let mut app = app_for_autofire();
        app.settings
            .set_autofire(
                &InputScope::Global,
                AutofireTarget {
                    player: 1,
                    action: BindingAction::A,
                },
                AutofireOverride::enabled(AutofirePattern {
                    period_frames: 2,
                    on_frames: 1,
                }),
            )
            .unwrap();
        app.sync_effective_input();
        app.host_input.set_remote(HostButton::A, true);
        app.observe_autofire_host_transitions();
        app.speed.paused = true;
        app.debug_requests.frame_advance = true;
        app.tick();
        assert_eq!(app.recording.pending_replay_batches.len(), 1);

        let directory = crate::test_support::test_directory("autofire-replay-start-order").unwrap();
        app.start_replay_recording_to_path(directory.path().join("capture.zrpl"))
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let response = loop {
            match app.emu_thread.as_ref().unwrap().poll_response() {
                EmuResponsePoll::Response(response) => break *response,
                EmuResponsePoll::Empty if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                EmuResponsePoll::Empty => {
                    panic!("capture response should arrive after the queued frame")
                }
                EmuResponsePoll::Disconnected => panic!("emulator worker disconnected"),
            }
        };
        assert!(app.consume_replay_start_response(response).is_none());
        assert_eq!(app.autofire_state.configured[0][0], 1);
        assert!(app.recording.pending_replay_batches.is_empty());
        assert_eq!(
            app.recording
                .replay_recorder
                .as_ref()
                .unwrap()
                .frame_count(),
            0
        );

        let (buttons, dpad) = app.current_host_joypad_input();
        let post_capture = app
            .prepare_replay_joypad_batch(1, buttons, dpad, (0.0, 0.0), None)
            .unwrap();
        assert_eq!(post_capture[0].buttons, 0);
        assert!(app.recording.pending_replay_batches.front().unwrap().record);
    }

    #[test]
    fn release_and_repress_between_staged_frames_rearms_to_on() {
        let mut app = app_for_autofire();
        app.settings
            .set_autofire(
                &InputScope::Global,
                AutofireTarget {
                    player: 1,
                    action: BindingAction::A,
                },
                AutofireOverride::enabled(AutofirePattern {
                    period_frames: 2,
                    on_frames: 1,
                }),
            )
            .unwrap();
        app.sync_effective_input();
        app.host_input.set_remote(HostButton::A, true);
        app.observe_autofire_host_transitions();
        let (buttons, dpad) = app.current_host_joypad_input();
        assert_eq!(
            app.prepare_replay_joypad_batch(1, buttons, dpad, (0.0, 0.0), None)
                .unwrap()[0]
                .buttons,
            1
        );

        app.host_input.set_remote(HostButton::A, false);
        app.observe_autofire_host_transitions();
        app.host_input.set_remote(HostButton::A, true);
        app.observe_autofire_host_transitions();
        app.commit_replay_batch(1);

        let (buttons, dpad) = app.current_host_joypad_input();
        assert_eq!(
            app.prepare_replay_joypad_batch(1, buttons, dpad, (0.0, 0.0), None)
                .unwrap()[0]
                .buttons,
            1
        );
    }

    #[test]
    fn releasing_one_target_preserves_other_target_phase() {
        let mut app = app_for_autofire();
        let pattern = AutofirePattern {
            period_frames: 2,
            on_frames: 1,
        };
        for action in [BindingAction::A, BindingAction::B] {
            app.settings
                .set_autofire(
                    &InputScope::Global,
                    AutofireTarget { player: 1, action },
                    AutofireOverride::enabled(pattern),
                )
                .unwrap();
        }
        app.sync_effective_input();
        app.autofire_rearm_pending = false;
        app.autofire_state.configured[0][0] = 1;
        app.autofire_state.configured[0][1] = 1;
        app.host_input.set_remote(HostButton::A, true);
        app.host_input.set_remote(HostButton::B, true);
        app.observe_autofire_host_transitions();
        app.host_input.set_remote(HostButton::B, false);
        app.observe_autofire_host_transitions();
        app.consume_autofire_rearm_if_idle();
        assert_eq!(app.autofire_state.configured[0][0], 1);
        assert_eq!(app.autofire_state.configured[0][1], 0);
    }

    #[test]
    fn resolved_input_changes_rearm_autofire_before_the_next_frame() {
        let mut app = app_for_autofire();
        app.autofire_state.configured[0][0] = 1;
        app.autofire_rearm_pending = false;
        app.settings
            .set_binding(
                &InputScope::Global,
                crate::settings::BindingTarget::Joypad {
                    player: 1,
                    action: BindingAction::A,
                },
                crate::settings::GameplayBindingSource::Keyboard,
                Some(crate::settings::PhysicalBinding::Keyboard(
                    winit::keyboard::KeyCode::KeyQ,
                )),
            )
            .unwrap();
        app.sync_effective_input();
        assert!(app.autofire_rearm_pending);
    }

    #[test]
    fn fault_commits_the_executed_autofire_prefix_before_teardown() {
        let mut app = app_for_autofire();
        app.settings
            .set_autofire(
                &InputScope::Global,
                AutofireTarget {
                    player: 1,
                    action: BindingAction::A,
                },
                AutofireOverride::enabled(AutofirePattern {
                    period_frames: 2,
                    on_frames: 1,
                }),
            )
            .unwrap();
        app.sync_effective_input();
        app.host_input.set_remote(HostButton::A, true);
        let (buttons, dpad) = app.current_host_joypad_input();
        app.prepare_replay_joypad_batch(2, buttons, dpad, (0.0, 0.0), None)
            .unwrap();
        app.process_frame_result(FrameResult {
            advanced_frames: 1,
            completed_step_requests: 1,
            staged_input_frames: 1,
            delivery_merged: false,
            replay_events: Vec::new(),
            replay_error: None,
            runtime_fault: Some("synthetic fault".into()),
            rumble: false,
            audio_samples: Vec::new(),
            audio_playback_speed: 1,
            ui_data: Default::default(),
            is_mbc7: false,
            is_gba_tilt: false,
            is_pocket_camera: false,
            game_boy_serial_device: None,
            game_boy_printer_jobs: Vec::new(),
            media_slot_snapshot: None,
            rewind_fill: 0.0,
            audio_semantic_frames: Vec::new(),
            audio_timeline_discontinuities: Vec::new(),
        });
        assert_eq!(app.autofire_state.configured[0][0], 1);
        assert!(app.recording.pending_replay_batches.is_empty());
    }

    #[test]
    fn replay_playback_bypasses_live_autofire_and_does_not_stage_phase() {
        let mut app = app_for_autofire();
        app.settings
            .set_autofire(
                &InputScope::Global,
                AutofireTarget {
                    player: 1,
                    action: BindingAction::A,
                },
                AutofireOverride::enabled(AutofirePattern {
                    period_frames: 2,
                    on_frames: 1,
                }),
            )
            .unwrap();
        app.sync_effective_input();
        app.host_input.set_remote(HostButton::A, true);
        let directory = crate::test_support::test_directory("autofire-replay-bypass").unwrap();
        let path = directory.path().join("input.zrpl");
        let mut recorder = zeff_emu_common::replay::ReplayRecorder::new(path.clone(), Vec::new());
        recorder.record_joypad_frame(ReplayJoypadFrame::p1(0xA5, 0x0C));
        recorder.finish().unwrap();
        app.recording.replay_player =
            Some(zeff_emu_common::replay::ReplayPlayer::load(&path).unwrap());

        let (buttons, dpad) = app.current_host_joypad_input();
        let frames = app
            .prepare_replay_joypad_batch(1, buttons, dpad, (0.0, 0.0), None)
            .unwrap();
        assert_eq!((frames[0].buttons, frames[0].dpad), (0xA5, 0x0C));
        assert_eq!(
            app.autofire_state,
            crate::app::autofire::AutofireState::default()
        );
        assert!(
            app.recording.pending_replay_batches[0]
                .autofire_states
                .is_empty()
        );
    }
}
