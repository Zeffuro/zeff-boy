use super::{App, MAX_IN_FLIGHT, SpeedMode, UI_RENDER_INTERVAL, VIEWER_UPDATE_INTERVAL};
use crate::debug::{self, DebugTab, DebugUiActions, is_tab_open};
use crate::emu_thread::{AudioConfig, EmuCommand, FrameInput, JoypadInput};
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
            if self.frames_in_flight < MAX_IN_FLIGHT {
                let now = Instant::now();
                let frames_to_step = if self.speed.paused {
                    self.timing.last_frame_time = now;
                    if std::mem::take(&mut self.debug_requests.frame_advance) {
                        1
                    } else {
                        0
                    }
                } else {
                    self.compute_frames_to_step(now)
                };

                let remote_capture_pending = self.remote_debug_frames_remaining > 0
                    || self.remote_memory_frames_remaining > 0
                    || self.remote_graphics_frames_remaining > 0;
                let has_pending = self.debug_requests.has_pending()
                    || self.pending_debug_actions.has_pending()
                    || remote_capture_pending;

                if frames_to_step > 0 || has_pending {
                    let want_viewer_update = match self.speed_mode() {
                        SpeedMode::Normal | SpeedMode::SlowMotion => true,
                        SpeedMode::FastForward | SpeedMode::Uncapped => {
                            let now = Instant::now();
                            if now.duration_since(self.timing.last_viewer_update)
                                >= VIEWER_UPDATE_INTERVAL
                            {
                                self.timing.last_viewer_update = now;
                                true
                            } else {
                                false
                            }
                        }
                    };

                    let host_camera_frame = self.camera_frame();
                    let (buttons_pressed, dpad_pressed) = self.gather_joypad_input();
                    let zapper = self.nes_zapper_input();
                    let reqs = debug::compute_tab_requirements(&self.debug_dock);
                    let snapshot = self.build_snapshot_request(&reqs, want_viewer_update);
                    let buffers = self.take_reusable_buffers();

                    let input = FrameInput {
                        frames: frames_to_step,
                        host_tilt,
                        host_camera_frame,
                        joypad: JoypadInput {
                            buttons: buttons_pressed,
                            dpad: dpad_pressed,
                            buttons_p2: 0,
                            dpad_p2: 0,
                        },
                        zapper,
                        debug_step: std::mem::take(&mut self.debug_requests.step),
                        debug_continue: std::mem::take(&mut self.debug_requests.continue_),
                        audio: AudioConfig {
                            apu_capture_enabled: reqs.needs_apu,
                            skip_audio: match self.speed_mode() {
                                SpeedMode::Uncapped => true,
                                SpeedMode::FastForward => {
                                    self.settings.audio.mute_during_fast_forward
                                }
                                SpeedMode::Normal | SpeedMode::SlowMotion => false,
                            },
                            midi_capture_active: self
                                .recording
                                .audio_recorder
                                .as_ref()
                                .is_some_and(|r| r.is_midi()),
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
                        if self.debug_windows.cheat.cheats_dirty {
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

                    if frames_to_step > 0 {
                        self.fps_tracker.tick_n(frames_to_step);
                    }
                }
            }
        }

        let should_render = match self.speed_mode() {
            SpeedMode::Normal | SpeedMode::SlowMotion => true,
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
        self.cached_ui_data = ui_frame_data;
        if !rendered {
            return;
        }

        self.debug_windows.tile_viewer_was_open =
            is_tab_open(&self.debug_dock, DebugTab::TileViewer);
        self.debug_windows.tilemap_viewer_was_open =
            is_tab_open(&self.debug_dock, DebugTab::TilemapViewer);
    }
}
