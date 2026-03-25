use super::{App, MAX_FRAMES_PER_TICK, SpeedMode};
use crate::graphics;
use std::time::Instant;

impl App {
    pub(super) fn update_debug_cache_edges(&mut self) {
        if self.debug_windows.show_tile_viewer && !self.tile_viewer_was_open {
            self.debug_windows.tiles.invalidate_cache();
        }
        if self.debug_windows.show_tilemap_viewer && !self.tilemap_viewer_was_open {
            self.debug_windows.tilemap.invalidate_cache();
        }
    }

    pub(super) fn sync_speed_setting(&mut self) {
        if self.timing.uncapped_speed != self.settings.uncapped_speed {
            self.timing.uncapped_speed = self.settings.uncapped_speed;
            if let Some(thread) = &self.emu_thread {
                thread.send(crate::emu_thread::EmuCommand::SetUncapped(
                    self.timing.uncapped_speed,
                ));
            }
        }
    }

    pub(super) fn poll_gamepad(&mut self) {
        if let Some(gamepad) = &mut self.gamepad {
            let poll = gamepad.poll(&self.settings.gamepad_bindings);

            if let Some(action) = self.debug_windows.rebinding_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set(action, button_name);
                    self.debug_windows.rebinding_gamepad = None;
                }
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_action {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set_action(action, button_name);
                    self.debug_windows.rebinding_gamepad_action = None;
                }
            } else {
                for (key, pressed) in poll.events {
                    self.host_input.set_gamepad(key, pressed);
                }
                for (action, pressed) in poll.action_events {
                    match action {
                        crate::settings::GamepadAction::SpeedUp => {
                            self.fast_forward_held = pressed;
                        }
                        crate::settings::GamepadAction::Rewind => {
                            self.rewind.held = pressed;
                        }
                        crate::settings::GamepadAction::Pause => {
                            if pressed {
                                self.paused = !self.paused;
                                self.toast_manager.set_persistent(
                                    "paused",
                                    self.paused,
                                    "⏸ Paused",
                                    egui::Color32::from_rgba_unmultiplied(50, 50, 90, 220),
                                    false,
                                );
                            }
                        }
                        crate::settings::GamepadAction::Turbo => {
                            self.turbo_held = pressed;
                        }
                    }
                }
            }

            self.left_stick = poll.left_stick;
        }
    }

    pub(super) fn compute_frames_to_step(&mut self, now: Instant) -> usize {
        match self.speed_mode() {
            SpeedMode::Uncapped => {
                self.timing.last_frame_time = now;
                1
            }
            SpeedMode::Normal | SpeedMode::FastForward => {
                let effective_duration = self.effective_frame_duration();

                let mut frames = 0usize;
                while self.timing.last_frame_time + effective_duration <= now {
                    frames += 1;
                    self.timing.last_frame_time += effective_duration;
                    if frames >= MAX_FRAMES_PER_TICK {
                        if self.settings.frame_skip {
                            self.timing.last_frame_time = now;
                        }
                        break;
                    }
                }
                frames
            }
        }
    }

    pub(super) fn drain_emu_responses(&mut self) {
        loop {
            let result = match &self.emu_thread {
                Some(thread) => thread.try_recv_frame(),
                None => return,
            };
            match result {
                Some(frame_result) => self.process_frame_result(frame_result),
                None => break,
            }
        }

        if self.rewind.pending || self.rewind.backstep_pending {
            while let Some(resp) = self.emu_thread.as_ref().and_then(|t| t.try_recv_response()) {
                match resp {
                    crate::emu_thread::EmuResponse::RewindOk { framebuffer } => {
                        self.latest_frame = Some(framebuffer);
                        if self.rewind.backstep_pending {
                            self.rewind.backstep_pending = false;
                            self.paused = true;
                            self.timing.last_frame_time = std::time::Instant::now();
                            self.toast_manager.set_persistent(
                                "paused",
                                true,
                                "⏸ Paused",
                                egui::Color32::from_rgba_unmultiplied(50, 50, 90, 220),
                                false,
                            );
                            self.toast_manager.info("⏮ Stepped back");
                        } else {
                            self.rewind.pending = false;
                            self.rewind.pops += 1;
                        }
                    }
                    crate::emu_thread::EmuResponse::RewindFailed(msg) => {
                        if self.rewind.backstep_pending {
                            self.rewind.backstep_pending = false;
                            self.toast_manager.info(format!("Can't step back: {msg}"));
                        } else {
                            self.rewind.pending = false;
                            log::debug!("Rewind: {}", msg);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub(super) fn render_frame(&mut self, ui_frame_data: Option<&crate::ui::UiFrameData>) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };

        let settings_was_open = self.show_settings_window;

        let speed_label = ui_frame_data
            .and_then(|d| d.debug_info.as_ref())
            .map(|info| info.speed_mode_label);

        let is_recording = self.recording.audio_recorder.is_some();
        let is_recording_replay = self.recording.replay_recorder.is_some();
        let is_playing_replay = self.recording.replay_player.is_some();
        let is_rewinding = self.rewind.held && self.settings.rewind_enabled;
        let autohide_menu_bar = self.settings.autohide_menu_bar;
        let cursor_y = self.cursor_pos.map(|(_, y)| y);
        let rewind_seconds_back =
            self.rewind.pops as f32 * self.settings.rewind_capture_interval() as f32 / 60.0;
        let slot_labels = super::state_io::build_slot_labels(self.cached_rom_hash);

        match gfx.render(graphics::RenderContext {
            debug_info: ui_frame_data.and_then(|d| d.debug_info.as_ref()),
            viewer_data: ui_frame_data.and_then(|d| d.viewer_data.as_ref()),
            rom_info_view: ui_frame_data.and_then(|d| d.rom_info_view.as_ref()),
            disassembly_view: ui_frame_data.and_then(|d| d.disassembly_view.as_ref()),
            memory_page: ui_frame_data.and_then(|d| d.memory_page.as_deref()),
            rom_page: ui_frame_data.and_then(|d| d.rom_page.as_deref()),
            rom_size: ui_frame_data.map_or(0, |d| d.rom_size),
            debug_windows: &mut self.debug_windows,
            settings: &mut self.settings,
            show_settings_window: &mut self.show_settings_window,
            dock_state: &mut self.debug_dock,
            toast_manager: &mut self.toast_manager,
            speed_mode_label: speed_label,
            is_recording_audio: is_recording,
            is_recording_replay,
            is_playing_replay,
            is_rewinding,
            rewind_seconds_back,
            is_paused: self.paused,
            autohide_menu_bar,
            cursor_y,
            slot_labels,
        }) {
            Ok(result) => {
                if result.open_file_requested {
                    self.open_file_dialog();
                }
                if result.reset_game_requested {
                    self.reset_game();
                }
                if result.stop_game_requested {
                    self.stop_game();
                }
                if result.save_state_file_requested {
                    self.save_state_file_dialog();
                }
                if result.load_state_file_requested {
                    self.load_state_file_dialog();
                }
                if let Some(slot) = result.save_state_slot {
                    self.save_state_slot(slot);
                }
                if let Some(slot) = result.load_state_slot {
                    self.load_state_slot(slot);
                }
                if let Some(path) = result.load_recent_rom {
                    self.load_rom(&path);
                }
                if result.toggle_fullscreen {
                    self.toggle_fullscreen();
                }
                if result.toggle_pause {
                    self.paused = !self.paused;
                    self.toast_manager.set_persistent(
                        "paused",
                        self.paused,
                        "⏸ Paused",
                        egui::Color32::from_rgba_unmultiplied(50, 50, 90, 220),
                        false,
                    );
                }
                if result.speed_change != 0 {
                    let mult = self.settings.fast_forward_multiplier as i32 + result.speed_change;
                    self.settings.fast_forward_multiplier = mult.clamp(1, 16) as usize;
                    self.settings.save();
                }
                if result.start_audio_recording {
                    self.start_audio_recording();
                }
                if result.stop_audio_recording {
                    self.stop_audio_recording();
                }
                if result.start_replay_recording {
                    self.start_replay_recording();
                }
                if result.stop_replay_recording {
                    self.stop_replay_recording();
                }
                if result.load_replay {
                    self.load_and_play_replay();
                }
                if result.take_screenshot {
                    self.take_screenshot();
                }
                crate::ui::apply_debug_actions(
                    &result.debug_actions,
                    &mut self.debug_requests.step,
                    &mut self.debug_requests.continue_,
                    &mut self.debug_requests.backstep,
                );
                self.merge_debug_actions(result.debug_actions);
                if let Some(toggles) = result.layer_toggles {
                    self.pending_debug_actions.layer_toggles = Some(toggles);
                }
                if !self.show_settings_window {
                    self.debug_windows.rebinding_action = None;
                    self.debug_windows.rebinding_shortcut = None;
                    self.debug_windows.rebinding_gamepad = None;
                    self.debug_windows.rebinding_gamepad_action = None;
                    self.debug_windows.rebinding_speedup = false;
                    self.debug_windows.rebinding_rewind = false;
                }
                if result.toolbar_settings_changed {
                    self.settings.save();
                }
                self.egui_wants_keyboard = result.egui_wants_keyboard;
            }
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Timeout) => {}
            Err(graphics::FrameError::OutOfMemory) => self.exit_requested = true,
        }

        if settings_was_open && !self.show_settings_window {
            self.settings.save();
        }

        if self.settings.vsync_mode != self.timing.last_vsync_mode {
            self.timing.last_vsync_mode = self.settings.vsync_mode;
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_vsync(self.settings.vsync_mode);
            }
        }

        true
    }

    fn merge_debug_actions(&mut self, actions: crate::debug::DebugUiActions) {
        let pending = &mut self.pending_debug_actions;
        if actions.add_breakpoint.is_some() {
            pending.add_breakpoint = actions.add_breakpoint;
        }
        if actions.add_watchpoint.is_some() {
            pending.add_watchpoint = actions.add_watchpoint;
        }
        pending
            .remove_breakpoints
            .extend(actions.remove_breakpoints);
        pending
            .toggle_breakpoints
            .extend(actions.toggle_breakpoints);
        pending.memory_writes.extend(actions.memory_writes);
        if actions.apu_channel_mutes.is_some() {
            pending.apu_channel_mutes = actions.apu_channel_mutes;
        }
    }
}
