use super::App;
use crate::debug::{DebugUiActions, MenuAction};
use crate::graphics;

impl App {
    pub(super) fn render_frame(&mut self, ui_frame_data: Option<&crate::ui::UiFrameData>) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };

        let settings_was_open = self.show_settings_window;

        let speed_label = ui_frame_data
            .and_then(|d| d.perf_info.as_ref())
            .map(|info| info.speed_mode_label.as_str());

        let is_recording = self.recording.is_audio_recording();
        let is_recording_replay = self.recording.replay_recorder.is_some();
        let is_playing_replay = self.recording.replay_player.is_some();
        let is_rewinding = self.rewind.held && self.settings.rewind.enabled;
        let autohide_menu_bar = self.settings.ui.autohide_menu_bar;
        let cursor_y = self.cursor_pos.map(|(_, y)| y);
        let rewind_seconds_back =
            self.rewind.pops as f32 * self.settings.rewind.capture_interval() as f32 / 60.0;
        let slot_labels = self.cached_slot_info.labels.clone();
        let slot_occupied = self.cached_slot_info.occupied;

        match gfx.render(graphics::RenderContext {
            cpu_debug: ui_frame_data.and_then(|d| d.cpu_debug.as_ref()),
            perf_info: ui_frame_data.and_then(|d| d.perf_info.as_ref()),
            apu_debug: ui_frame_data.and_then(|d| d.apu_debug.as_ref()),
            oam_debug: ui_frame_data.and_then(|d| d.oam_debug.as_ref()),
            palette_debug: ui_frame_data.and_then(|d| d.palette_debug.as_ref()),
            rom_debug: ui_frame_data.and_then(|d| d.rom_debug.as_ref()),
            input_debug: ui_frame_data.and_then(|d| d.input_debug.as_ref()),
            graphics_data: ui_frame_data.and_then(|d| d.graphics_data.as_ref()),
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
            is_paused: self.speed.paused,
            is_pocket_camera: self.rom_info.is_pocket_camera,
            autohide_menu_bar,
            cursor_y,
            slot_labels,
            slot_occupied,
            active_save_slot: self.active_save_slot,
        }) {
            Ok(result) => {
                let mut settings_dirty = false;
                for action in &result.actions {
                    match action {
                        MenuAction::OpenFile => self.open_file_dialog(),
                        MenuAction::ResetGame => self.reset_game(),
                        MenuAction::StopGame => self.stop_game(),
                        MenuAction::SaveStateFile => self.save_state_file_dialog(),
                        MenuAction::LoadStateFile => self.load_state_file_dialog(),
                        MenuAction::SaveStateSlot(slot) => self.save_state_slot(*slot),
                        MenuAction::LoadStateSlot(slot) => self.load_state_slot(*slot),
                        MenuAction::LoadRecentRom(path) => self.load_rom(path),
                        MenuAction::ToggleFullscreen => self.toggle_fullscreen(),
                        MenuAction::TogglePause => {
                            self.speed.paused = !self.speed.paused;
                            self.toast_manager.set_paused(self.speed.paused);
                        }
                        MenuAction::SpeedChange(delta) => {
                            let mult =
                                self.settings.emulation.fast_forward_multiplier as i32 + delta;
                            self.settings.emulation.fast_forward_multiplier =
                                mult.clamp(1, 16) as usize;
                            settings_dirty = true;
                        }
                        MenuAction::StartAudioRecording => {
                            self.start_audio_recording();
                        }
                        MenuAction::StopAudioRecording => {
                            self.stop_audio_recording();
                        }
                        MenuAction::StartReplayRecording => self.start_replay_recording(),
                        MenuAction::StopReplayRecording => self.stop_replay_recording(),
                        MenuAction::LoadReplay => self.load_and_play_replay(),
                        MenuAction::TakeScreenshot => self.take_screenshot(),
                        MenuAction::ToolbarSettingsChanged => settings_dirty = true,
                        MenuAction::SetLayerToggles(bg, win, sprites) => {
                            self.pending_debug_actions.layer_toggles = Some((*bg, *win, *sprites));
                        }
                        MenuAction::SetAspectRatio(_) | MenuAction::OpenSettings => {}
                    }
                }
                if settings_dirty {
                    self.settings.save();
                }
                crate::ui::apply_debug_actions(
                    &result.debug_actions,
                    &mut self.debug_requests.step,
                    &mut self.debug_requests.continue_,
                    &mut self.debug_requests.backstep,
                );
                self.merge_debug_actions(result.debug_actions);
                if !self.show_settings_window {
                    self.debug_windows.rebinding_action = None;
                    self.debug_windows.rebinding_shortcut = None;
                    self.debug_windows.rebinding_gamepad = None;
                    self.debug_windows.rebinding_gamepad_action = None;
                    self.debug_windows.rebinding_speedup = false;
                    self.debug_windows.rebinding_rewind = false;
                }
                self.egui_wants_keyboard = result.egui_wants_keyboard;
                self.game_view_focused = result.game_view_focused;
            }
            Err(graphics::FrameError::Outdated | graphics::FrameError::Lost) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Timeout) => {}
        }

        if settings_was_open && !self.show_settings_window {
            self.settings.save();
        }

        if self.settings.video.vsync_mode != self.timing.last_vsync_mode {
            self.timing.last_vsync_mode = self.settings.video.vsync_mode;
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_vsync(self.settings.video.vsync_mode);
            }
        }

        true
    }

    fn merge_debug_actions(&mut self, actions: DebugUiActions) {
        let pending = &mut self.pending_debug_actions;
        if actions.add_breakpoint.is_some() {
            pending.add_breakpoint = actions.add_breakpoint;
        }
        if actions.add_watchpoint.is_some() {
            pending.add_watchpoint = actions.add_watchpoint;
        }
        let bp_changed =
            !actions.remove_breakpoints.is_empty() || !actions.toggle_breakpoints.is_empty();
        pending
            .remove_breakpoints
            .extend(actions.remove_breakpoints);
        pending
            .toggle_breakpoints
            .extend(actions.toggle_breakpoints);
        if bp_changed || actions.add_breakpoint.is_some() {
            self.debug_windows.last_disasm_pc = None;
        }
        pending.memory_writes.extend(actions.memory_writes);
        if actions.apu_channel_mutes.is_some() {
            pending.apu_channel_mutes = actions.apu_channel_mutes;
        }
    }
}
