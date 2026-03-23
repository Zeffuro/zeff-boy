use super::{App, MAX_FRAMES_PER_TICK, SpeedMode};
use crate::graphics;
use std::time::Instant;

impl App {
    pub(super) fn update_debug_cache_edges(&mut self) {
        if self.debug_windows.show_tile_viewer && !self.tile_viewer_was_open {
            self.debug_windows.invalidate_tile_viewer_cache();
        }
        if self.debug_windows.show_tilemap_viewer && !self.tilemap_viewer_was_open {
            self.debug_windows.invalidate_tilemap_cache();
        }
    }

    pub(super) fn sync_speed_setting(&mut self) {
        if self.uncapped_speed != self.settings.uncapped_speed {
            self.uncapped_speed = self.settings.uncapped_speed;
            if let Some(thread) = &self.emu_thread {
                thread.send(crate::emu_thread::EmuCommand::SetUncapped(self.uncapped_speed));
            }
        }
    }

    pub(super) fn poll_gamepad(&mut self) {
        if let Some(gamepad) = &mut self.gamepad {
            let poll = gamepad.poll();
            for (key, pressed) in poll.events {
                self.host_input.set_gamepad(key, pressed);
            }
            self.left_stick = poll.left_stick;
        }
    }

    pub(super) fn compute_frames_to_step(&mut self, now: Instant) -> usize {
        match self.speed_mode() {
            SpeedMode::Uncapped => {
                self.last_frame_time = now;
                1
            }
            SpeedMode::Normal | SpeedMode::FastForward => {
                let effective_duration = self.effective_frame_duration();

                let mut frames = 0usize;
                while self.last_frame_time + effective_duration <= now {
                    frames += 1;
                    self.last_frame_time += effective_duration;
                    if frames >= MAX_FRAMES_PER_TICK {
                        if self.settings.frame_skip {
                            self.last_frame_time = now;
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
    }

    pub(super) fn render_frame(
        &mut self,
        ui_frame_data: Option<&crate::ui::UiFrameData>,
    ) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };


        let settings_before = if self.show_settings_window {
            Some(self.settings.clone())
        } else {
            None
        };

        let speed_label = ui_frame_data
            .and_then(|d| d.debug_info.as_ref())
            .map(|info| info.speed_mode_label);

        let is_recording = self.audio_recorder.is_some();
        let is_recording_replay = self.replay_recorder.is_some();
        let is_playing_replay = self.replay_player.is_some();
        let is_rewinding = self.rewind_held && self.settings.rewind_enabled;

        match gfx.render(
            ui_frame_data.and_then(|d| d.debug_info.as_ref()),
            ui_frame_data.and_then(|d| d.viewer_data.as_ref()),
            ui_frame_data
                .and_then(|d| d.rom_info_view.as_ref()),
            ui_frame_data
                .and_then(|d| d.disassembly_view.as_ref()),
            ui_frame_data
                .and_then(|d| d.memory_page.as_deref()),
            &mut self.debug_windows,
            &mut self.settings,
            &mut self.show_settings_window,
            &mut self.debug_dock,
            &mut self.toast_manager,
            speed_label,
            is_recording,
            is_recording_replay,
            is_playing_replay,
            is_rewinding,
        ) {
            Ok(result) => {
                if result.open_file_requested {
                    self.open_file_dialog();
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
                    &mut self.debug_step_requested,
                    &mut self.debug_continue_requested,
                );
                self.merge_debug_actions(result.debug_actions);
                if !self.show_settings_window {
                    self.debug_windows.rebinding_action = None;
                    self.debug_windows.rebinding_speedup = false;
                    self.debug_windows.rebinding_rewind = false;
                }
                if result.toolbar_settings_changed {
                    self.settings.save();
                }
            }
            Err(graphics::FrameError::Outdated) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Lost) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Timeout) => {}
            Err(graphics::FrameError::OutOfMemory) => self.exit_requested = true,
        }

        if let Some(prev) = settings_before {
            if self.settings != prev {
                self.settings.save();
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
        pending.remove_breakpoints.extend(actions.remove_breakpoints);
        pending.toggle_breakpoints.extend(actions.toggle_breakpoints);
        pending.memory_writes.extend(actions.memory_writes);
        if actions.apu_channel_mutes.is_some() {
            pending.apu_channel_mutes = actions.apu_channel_mutes;
        }
    }
}
