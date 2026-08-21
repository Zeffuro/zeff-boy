use super::{
    App, MAX_IN_FLIGHT, SETTINGS_UPDATE_INTERVAL, SpeedMode, UI_RENDER_INTERVAL,
    VIEWER_UPDATE_INTERVAL,
};
use crate::debug::{self, DebugTab, DebugUiActions, is_tab_open};
use crate::emu_thread::{AudioConfig, AudioRecordingCapture, EmuCommand, FrameInput, JoypadInput};
use crate::platform::Instant;

mod input;
mod nes_palette;
mod search;
mod snapshot;
mod timing;
mod zapper;

impl App {
    pub(super) fn update_debug_cache_edges(&mut self) {
        if is_tab_open(&self.debug_dock, DebugTab::TileViewer)
            && !self.debug_windows.tile_viewer_was_open
        {
            self.debug_windows.tiles.invalidate_cache();
        }
        if is_tab_open(&self.debug_dock, DebugTab::TilemapViewer)
            && !self.debug_windows.tilemap_viewer_was_open
        {
            self.debug_windows.tilemap.invalidate_cache();
        }
    }

    pub(super) fn tick(&mut self) {
        self.sync_speed_setting();
        if self.last_audio_output_sample_rate != self.settings.audio.output_sample_rate {
            self.last_audio_output_sample_rate = self.settings.audio.output_sample_rate;
            self.reset_audio_output();
            self.settings.save();
        }
        self.poll_gamepad();

        let host_tilt = self.update_host_tilt_and_stick_mode();

        self.drain_emu_responses();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_symbol_load();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_replay_save_worker();

        // Handle backstep: pop one rewind snapshot and pause
        if std::mem::take(&mut self.debug_requests.backstep)
            && self.settings.rewind.enabled
            && !self.rewind.pending
            && !self.rewind.backstep_pending
            && let Some(thread) = &self.emu_thread
        {
            thread.send(EmuCommand::Rewind);
            self.rewind.backstep_pending = true;
        }

        if self.rewind.held && self.settings.rewind.enabled {
            self.rewind.throttle += 1;
            let pop_interval = self.settings.rewind.speed.max(1);
            if self.rewind.throttle >= pop_interval
                && !self.rewind.pending
                && !self.rewind.backstep_pending
            {
                self.rewind.throttle = 0;
                if let Some(thread) = &self.emu_thread {
                    thread.send(EmuCommand::Rewind);
                    self.rewind.pending = true;
                }
            }
        } else {
            if self.rewind.throttle > 0 {
                self.timing.last_frame_time = Instant::now();
                self.rewind.throttle = 0;
            }
            self.rewind.pops = 0;
            let max_in_flight = if self.recording.limits_in_flight_for_replay() {
                1
            } else {
                MAX_IN_FLIGHT
            };

            if self.frames_in_flight < max_in_flight {
                let now = Instant::now();
                let next_frame_requested = std::mem::take(&mut self.debug_requests.next_frame);
                let mut frames_to_step = if self.speed.paused {
                    self.timing.last_frame_time = now;
                    if std::mem::take(&mut self.debug_requests.frame_advance) {
                        1
                    } else {
                        0
                    }
                } else {
                    self.compute_frames_to_step(now)
                };
                if next_frame_requested {
                    frames_to_step = 1;
                }
                if frames_to_step > 0
                    && let Some(batch) = self.recording.pending_replay_batches.front()
                {
                    frames_to_step = frames_to_step.min(batch.frames.len());
                }
                if let Some(recorder) = self.recording.replay_recorder.as_ref() {
                    let until_checkpoint = 300 - recorder.frame_count() % 300;
                    frames_to_step = frames_to_step.min(until_checkpoint);
                }
                if let Some(player) = self.recording.replay_player.as_ref() {
                    let cursor = u64::try_from(player.cursor()).unwrap_or(u64::MAX);
                    if let Some(checkpoint) = player
                        .metadata()
                        .checkpoints
                        .iter()
                        .find(|checkpoint| checkpoint.frame > cursor)
                    {
                        frames_to_step = frames_to_step.min((checkpoint.frame - cursor) as usize);
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    frames_to_step = self.limit_frames_for_live_button_releases(frames_to_step);
                }

                let remote_capture_pending = self.remote_debug_frames_remaining > 0
                    || self.remote_memory_frames_remaining > 0
                    || self.remote_graphics_frames_remaining > 0;
                let has_pending = self.debug_requests.has_pending()
                    || self.pending_debug_actions.has_pending()
                    || remote_capture_pending;

                if frames_to_step > 0 || has_pending {
                    let throttle_viewers = self.active_debug_presentation
                        == crate::settings::DebugPresentation::GameAndDebugger
                        || matches!(
                            self.speed_mode(),
                            SpeedMode::FastForward | SpeedMode::Uncapped
                        );
                    let want_viewer_update = has_pending || !throttle_viewers || {
                        let now = Instant::now();
                        if now.duration_since(self.timing.last_viewer_update)
                            >= VIEWER_UPDATE_INTERVAL
                        {
                            self.timing.last_viewer_update = now;
                            true
                        } else {
                            false
                        }
                    };

                    let (buttons_pressed, dpad_pressed) =
                        self.current_replay_recordable_joypad_input();
                    let replay_playback_active = self.recording.replay_player.is_some();
                    let host_camera_frame = if replay_playback_active {
                        None
                    } else {
                        self.camera_frame()
                    };
                    if replay_playback_active {
                        self.apply_replay_events_at_cursor();
                    }
                    let replay_joypad_frames = self.prepare_replay_joypad_batch(
                        frames_to_step,
                        buttons_pressed,
                        dpad_pressed,
                        host_tilt,
                        host_camera_frame.as_deref(),
                    );
                    if replay_playback_active {
                        frames_to_step =
                            replay_joypad_frames.as_ref().map_or(0, std::vec::Vec::len);
                    }
                    let (buttons_pressed_p2, dpad_pressed_p2) =
                        replay_joypad_frames.as_ref().map_or_else(
                            || self.current_host_joypad_p2_input(),
                            |frames| {
                                frames
                                    .first()
                                    .map(|frame| (frame.buttons_p2, frame.dpad_p2))
                                    .unwrap_or((0, 0))
                            },
                        );
                    let zapper = replay_joypad_frames.as_ref().map_or_else(
                        || self.nes_zapper_input(),
                        |frames| {
                            frames
                                .first()
                                .map(|frame| frame.zapper.into())
                                .unwrap_or_default()
                        },
                    );
                    let host_tilt = replay_joypad_frames.as_ref().map_or(host_tilt, |frames| {
                        frames
                            .first()
                            .map(|frame| frame.host_tilt)
                            .unwrap_or((0.0, 0.0))
                    });
                    let host_camera_frame = replay_joypad_frames
                        .as_ref()
                        .map_or(host_camera_frame, |frames| {
                            frames.first().and_then(|frame| frame.camera_frame.clone())
                        });
                    let reqs = if self.debug_workspace_visible() {
                        debug::compute_tab_requirements(&self.debug_dock)
                    } else {
                        debug::dock::TabDataRequirements::default()
                    };
                    let snapshot = self.build_snapshot_request(&reqs, want_viewer_update);
                    let buffers = self.take_reusable_buffers();

                    let input = FrameInput {
                        frames: frames_to_step,
                        replay_joypad_frames,
                        host_tilt,
                        host_camera_frame,
                        joypad: JoypadInput {
                            buttons: buttons_pressed,
                            dpad: dpad_pressed,
                            buttons_p2: buttons_pressed_p2,
                            dpad_p2: dpad_pressed_p2,
                        },
                        zapper,
                        debug_step: std::mem::take(&mut self.debug_requests.step),
                        debug_continue: std::mem::take(&mut self.debug_requests.continue_)
                            || next_frame_requested,
                        debug_suspend_after_frame: next_frame_requested,
                        audio: AudioConfig {
                            apu_capture_enabled: reqs.needs_apu && want_viewer_update,
                            skip_audio: match self.speed_mode() {
                                SpeedMode::Uncapped => self
                                    .recording
                                    .audio_recorder
                                    .as_ref()
                                    .is_none_or(|recorder| recorder.captures_semantics()),
                                SpeedMode::FastForward => {
                                    self.settings.audio.mute_during_fast_forward
                                }
                                SpeedMode::Normal | SpeedMode::SlowMotion => false,
                            },
                            recording_capture: AudioRecordingCapture {
                                active: self.recording.audio_recorder.is_some(),
                                semantic: self
                                    .recording
                                    .audio_recorder
                                    .as_ref()
                                    .is_some_and(|recorder| recorder.captures_semantics()),
                            },
                        },
                        debug_actions: std::mem::replace(
                            &mut self.pending_debug_actions,
                            DebugUiActions::none(),
                        ),
                        snapshot,
                        buffers,
                        rewind_enabled: self.settings.rewind.enabled && !self.rewind.held,
                        rewind_seconds: self.settings.rewind.seconds,
                    };

                    if let Some(thread) = &self.emu_thread {
                        if self.debug_windows.cheat.cheats_dirty
                            && self.recording.allows_cheat_updates()
                        {
                            self.debug_windows.cheat.cheats_dirty = false;
                            thread.send(EmuCommand::UpdateCheats(
                                crate::cheats::collect_enabled_patches(
                                    &self.debug_windows.cheat.user_codes,
                                    &self.debug_windows.cheat.libretro_codes,
                                ),
                            ));
                        }
                        thread.send(EmuCommand::StepFrames(Box::new(input)));
                        self.frames_in_flight += 1;
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    if self.emu_thread.is_some() && frames_to_step > 0 {
                        self.advance_live_button_releases(frames_to_step);
                    }

                    if frames_to_step > 0 {
                        self.fps_tracker.tick_n(frames_to_step);
                    }
                }
            }
        }

        let should_render = match self.speed_mode() {
            SpeedMode::Normal | SpeedMode::SlowMotion => {
                let background_ui = self.emu_thread.is_none() || {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.show_settings_window
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        false
                    }
                };
                !background_ui
                    || Instant::now().duration_since(self.timing.last_render_time)
                        >= VIEWER_UPDATE_INTERVAL
            }
            SpeedMode::FastForward | SpeedMode::Uncapped => {
                let now = Instant::now();
                if now.duration_since(self.timing.last_render_time) >= UI_RENDER_INTERVAL {
                    self.timing.last_render_time = now;
                    true
                } else {
                    false
                }
            }
        };

        if !should_render {
            return;
        }
        self.timing.last_render_time = Instant::now();

        self.update_debug_cache_edges();

        if let Some(frame) = self.latest_frame.take() {
            self.last_core_frame = Some(frame.clone());
            let Some(display_frame) = self.display_frame_for_upload(frame) else {
                log::warn!(
                    "Skipping frame upload with unexpected WonderSwan size for {:?}",
                    self.active_system
                );
                return;
            };

            let Some((native_w, native_h)) = self.display_size_for_frame_len(display_frame.len())
            else {
                log::warn!(
                    "Skipping frame upload with unexpected size: {} bytes for {:?}",
                    display_frame.len(),
                    self.active_system
                );
                return;
            };

            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_native_size(native_w, native_h);
                gfx.upload_framebuffer(&display_frame);
            }
            self.last_displayed_frame = Some(display_frame);
        }

        self.debug_windows.memory.enable_editing = self.settings.ui.enable_memory_editing;

        let ui_frame_data = self.cached_ui_data.take();
        let rendered = self.render_frame(ui_frame_data.as_ref());
        #[cfg(not(target_arch = "wasm32"))]
        self.render_native_auxiliary_windows(ui_frame_data.as_ref());
        self.cached_ui_data = ui_frame_data;
        if !rendered {
            return;
        }

        self.debug_windows.tile_viewer_was_open =
            is_tab_open(&self.debug_dock, DebugTab::TileViewer);
        self.debug_windows.tilemap_viewer_was_open =
            is_tab_open(&self.debug_dock, DebugTab::TilemapViewer);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_native_auxiliary_windows(&mut self, data: Option<&crate::ui::UiFrameData>) {
        let now = Instant::now();
        let debugger_visible = self.active_debug_presentation
            == crate::settings::DebugPresentation::GameAndDebugger
            && self.settings.ui.debugger_window_open
            && self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::debugger_window)
                .is_some_and(|window| window.is_minimized() != Some(true));
        if debugger_visible
            && now.duration_since(self.last_debugger_render) >= VIEWER_UPDATE_INTERVAL
        {
            self.render_debugger_frame(data);
            self.last_debugger_render = now;
        }

        let settings_visible = self.show_settings_window
            && self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::settings_window)
                .is_some_and(|window| window.is_minimized() != Some(true));
        if settings_visible
            && now.duration_since(self.last_settings_render) >= SETTINGS_UPDATE_INTERVAL
        {
            let before = self.settings.clone();
            self.render_settings_frame(data);
            if self.settings != before {
                self.settings.save();
            }
            self.last_settings_render = now;
        }

        let printer_visible = self.show_printer_window
            && self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::printer_window)
                .is_some_and(|window| window.is_minimized() != Some(true));
        if printer_visible && now.duration_since(self.last_printer_render) >= VIEWER_UPDATE_INTERVAL
        {
            self.render_printer_frame();
            self.last_printer_render = now;
        }
    }
}
