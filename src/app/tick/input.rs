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
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_hidden_settings_capture();
        let supports_rewind = self.core_supports_rewind();
        let capture_active = self.debug_windows.rebinding_gamepad.is_some()
            || self.debug_windows.rebinding_gamepad_p2.is_some()
            || self.debug_windows.rebinding_gamepad_pce_multitap.is_some()
            || self.debug_windows.rebinding_ws_gamepad.is_some()
            || self.debug_windows.rebinding_gamepad_action.is_some()
            || self.debug_windows.rebinding_action.is_some()
            || self.debug_windows.rebinding_shortcut.is_some()
            || self.debug_windows.rebinding_speedup
            || self.debug_windows.rebinding_rewind;
        self.sync_keyboard_capture(capture_active);
        let Some(gamepad) = &mut self.gamepad else {
            self.debug_windows.settings_ui.gamepad_commands.clear();
            self.debug_windows.settings_ui.gamepad_snapshot = Default::default();
            return;
        };
        for command in self.debug_windows.settings_ui.gamepad_commands.drain(..) {
            gamepad.apply_command(command);
        }
        let poll = gamepad.poll(
            &self.settings.gamepad_bindings,
            &self.settings.input_devices,
            capture_active,
            self.settings.tilt.deadzone,
        );
        let snapshot = gamepad.snapshot();
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
        self.host_input
            .set_multiplayer_gamepad_sticks(poll.player_sticks, self.settings.tilt.deadzone);

        if let Some(button_name) = captured {
            if let Some(action) = self.debug_windows.rebinding_gamepad {
                self.settings.gamepad_bindings.set(action, button_name);
                self.debug_windows.rebinding_gamepad = None;
                self.debug_windows.rebinding_gamepad_p2 = None;
                self.debug_windows.rebinding_gamepad_pce_multitap = None;
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_p2 {
                self.settings.gamepad_bindings.set_p2(action, button_name);
                self.debug_windows.rebinding_gamepad = None;
                self.debug_windows.rebinding_gamepad_p2 = None;
                self.debug_windows.rebinding_gamepad_pce_multitap = None;
            } else if let Some((player, action)) = self.debug_windows.rebinding_gamepad_pce_multitap
            {
                self.settings
                    .gamepad_bindings
                    .set_for_player(action, player, button_name);
                self.debug_windows.rebinding_gamepad_pce_multitap = None;
            } else if let Some(button) = self.debug_windows.rebinding_ws_gamepad {
                self.settings.gamepad_bindings.set_ws(button, button_name);
                self.debug_windows.rebinding_ws_gamepad = None;
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_action {
                self.settings
                    .gamepad_bindings
                    .set_action(action, button_name);
                self.debug_windows.rebinding_gamepad_action = None;
            }
        }
    }

    pub(super) fn current_replay_recordable_joypad_input(&mut self) -> (u8, u8) {
        let (mut buttons, dpad) = self.current_host_joypad_input();
        if self.speed.turbo_held {
            self.speed.turbo_counter = self.speed.turbo_counter.wrapping_add(1);
            if self.speed.turbo_counter % 2 == 1 {
                buttons = 0;
            }
        } else {
            self.speed.turbo_counter = 0;
        }

        (buttons, dpad)
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
                });
            return Some(frames);
        }

        if self.recording.should_stage_replay_recording_input() {
            let (buttons_p2, dpad_p2) = self.current_host_joypad_p2_input();
            let (buttons_p3, dpad_p3) = self.current_host_joypad_p3_input();
            let (buttons_p4, dpad_p4) = self.current_host_joypad_p4_input();
            let (buttons_p5, dpad_p5) = self.current_host_joypad_p5_input();
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
            let frames = vec![
                ReplayJoypadFrame {
                    buttons,
                    dpad,
                    buttons_p2,
                    dpad_p2,
                    buttons_p3,
                    dpad_p3,
                    buttons_p4,
                    dpad_p4,
                    buttons_p5,
                    dpad_p5,
                    zapper,
                    host_tilt,
                    camera_frame,
                    coleco: Default::default(),
                };
                frames_to_step
            ];
            self.recording
                .pending_replay_batches
                .push_back(PendingReplayBatch {
                    frames: frames.clone(),
                    record: true,
                    playback: false,
                });
            return Some(frames);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::encode_pce_mouse_axis;

    #[test]
    fn pce_mouse_axis_uses_host_relative_sign_sensitivity_and_saturation() {
        assert_eq!(encode_pce_mouse_axis(8.0, 1.25), -10);
        assert_eq!(encode_pce_mouse_axis(-8.0, 1.25), 10);
        assert_eq!(encode_pce_mouse_axis(8.0, 0.0), -2);
        assert_eq!(encode_pce_mouse_axis(8.0, 99.0), -32);
        assert_eq!(encode_pce_mouse_axis(-1_000_000.0, 1.0), i16::MAX);
    }
}
