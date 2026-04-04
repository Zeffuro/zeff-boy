use super::{
    ActiveSystem, App, MAX_FRAMES_PER_TICK, MAX_IN_FLIGHT, SpeedMode, UI_RENDER_INTERVAL,
    VIEWER_UPDATE_INTERVAL,
};
use crate::debug::{self, DebugTab, DebugUiActions, is_tab_open};
use crate::emu_thread::{
    AudioConfig, EmuCommand, FrameInput, JoypadInput, MemorySearchRequest, RenderSettings,
    ReusableBuffers, SnapshotRequest,
};
use crate::settings::GamepadAction;
use std::time::Instant;

fn native_size_for_frame(system: ActiveSystem, frame_len: usize) -> Option<(u32, u32)> {
    const GB_FRAME_LEN: usize = 160 * 144 * 4;
    const SGB_FRAME_LEN: usize = 256 * 224 * 4;
    const NES_FRAME_LEN: usize = 256 * 240 * 4;

    match (system, frame_len) {
        (ActiveSystem::GameBoy, GB_FRAME_LEN) => Some((160, 144)),
        (ActiveSystem::GameBoy, SGB_FRAME_LEN) => Some((256, 224)),
        (ActiveSystem::Nes, NES_FRAME_LEN) => Some((256, 240)),
        _ => None,
    }
}

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

    pub(super) fn sync_speed_setting(&mut self) {
        if self.timing.uncapped_speed != self.settings.emulation.uncapped_speed {
            self.timing.uncapped_speed = self.settings.emulation.uncapped_speed;
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::SetUncapped(self.timing.uncapped_speed));
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
                    self.settings
                        .gamepad_bindings
                        .set_action(action, button_name);
                    self.debug_windows.rebinding_gamepad_action = None;
                }
            } else {
                for (key, pressed) in poll.events {
                    self.host_input.set_gamepad(key, pressed);
                }
                for (action, pressed) in poll.action_events {
                    match action {
                        GamepadAction::SpeedUp => {
                            self.speed.fast_forward_held = pressed;
                        }
                        GamepadAction::Rewind => {
                            self.rewind.held = pressed;
                        }
                        GamepadAction::Pause => {
                            if pressed {
                                self.speed.paused = !self.speed.paused;
                                self.toast_manager.set_paused(self.speed.paused);
                            }
                        }
                        GamepadAction::Turbo => {
                            self.speed.turbo_held = pressed;
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
                        if self.settings.emulation.frame_skip {
                            self.timing.last_frame_time = now;
                        }
                        break;
                    }
                }
                frames
            }
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

                let has_pending =
                    self.debug_requests.has_pending() || self.pending_debug_actions.has_pending();

                if frames_to_step > 0 || has_pending {
                    let host_camera_frame = self.camera_frame();
                    if let Some(thread) = &self.emu_thread {
                        let want_viewer_update = match self.speed_mode() {
                            SpeedMode::Normal => true,
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

                        let (mut buttons_pressed, dpad_pressed) =
                            if let Some(player) = &mut self.recording.replay_player {
                                if let Some((buttons, dpad)) = player.next_frame() {
                                    (buttons, dpad)
                                } else {
                                    self.toast_manager.info("Replay finished");
                                    self.recording.replay_player = None;
                                    (
                                        self.host_input.buttons_pressed(),
                                        self.host_input.dpad_pressed(),
                                    )
                                }
                            } else {
                                (
                                    self.host_input.buttons_pressed(),
                                    self.host_input.dpad_pressed(),
                                )
                            };

                        if self.speed.turbo_held {
                            self.speed.turbo_counter = self.speed.turbo_counter.wrapping_add(1);
                            if self.speed.turbo_counter % 2 == 1 {
                                buttons_pressed = 0;
                            }
                        } else {
                            self.speed.turbo_counter = 0;
                        }

                        if let Some(recorder) = &mut self.recording.replay_recorder {
                            recorder.record_frame(buttons_pressed, dpad_pressed);
                        }

                        let reqs = debug::compute_tab_requirements(&self.debug_dock);
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
                            debug_step: std::mem::take(&mut self.debug_requests.step),
                            debug_continue: std::mem::take(&mut self.debug_requests.continue_),
                            audio: AudioConfig {
                                apu_capture_enabled: reqs.needs_apu,
                                skip_audio: match self.speed_mode() {
                                    SpeedMode::Uncapped => true,
                                    SpeedMode::FastForward => {
                                        self.settings.audio.mute_during_fast_forward
                                    }
                                    SpeedMode::Normal => false,
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
                            snapshot: SnapshotRequest {
                                want_debug_info: (reqs.needs_debug_info
                                    || self.settings.ui.show_fps),
                                want_perf_info: reqs.needs_perf_info || self.settings.ui.show_fps,
                                any_viewer_open: reqs.needs_viewer_data && want_viewer_update,
                                any_vram_viewer_open: reqs.needs_vram && want_viewer_update,
                                show_oam_viewer: reqs.needs_oam && want_viewer_update,
                                show_apu_viewer: reqs.needs_apu && want_viewer_update,
                                show_disassembler: reqs.needs_disassembly && want_viewer_update,
                                show_rom_info: reqs.needs_rom_info && want_viewer_update,
                                show_memory_viewer: reqs.needs_memory_page && want_viewer_update,
                                memory_view_start: self.debug_windows.memory.view_start,
                                show_rom_viewer: reqs.needs_rom_page && want_viewer_update,
                                rom_view_start: self.debug_windows.rom_viewer.view_start,
                                last_disasm_pc: self.debug_windows.last_disasm_pc,
                                memory_search: if self.debug_windows.memory.search_pending {
                                    self.debug_windows.memory.search_pending = false;
                                    debug::hex_search::parse_search_query(
                                        &self.debug_windows.memory.search_query,
                                        self.debug_windows.memory.search_mode,
                                    )
                                    .map(|pattern| {
                                        MemorySearchRequest {
                                            pattern,
                                            max_results: self
                                                .debug_windows
                                                .memory
                                                .search_max_results,
                                        }
                                    })
                                } else {
                                    None
                                },
                                rom_search: if self.debug_windows.rom_viewer.search_pending {
                                    self.debug_windows.rom_viewer.search_pending = false;
                                    debug::hex_search::parse_search_query(
                                        &self.debug_windows.rom_viewer.search_query,
                                        self.debug_windows.rom_viewer.search_mode,
                                    )
                                    .map(|pattern| {
                                        MemorySearchRequest {
                                            pattern,
                                            max_results: self
                                                .debug_windows
                                                .rom_viewer
                                                .search_max_results,
                                        }
                                    })
                                } else {
                                    None
                                },
                                render: RenderSettings {
                                    color_correction: self.settings.video.color_correction,
                                    color_correction_matrix: self
                                        .settings
                                        .video
                                        .color_correction_matrix,
                                    dmg_palette_preset: self.settings.video.dmg_palette_preset,
                                    nes_palette_mode: self.settings.video.nes_palette_mode,
                                    sgb_border_enabled: self.settings.emulation.sgb_border_enabled,
                                },
                            },
                            buffers: ReusableBuffers {
                                framebuffer: self.recycled.framebuffer.take(),
                                audio: self.recycled.audio.take(),
                                vram: self.recycled.vram.take(),
                                oam: self.recycled.oam.take(),
                                memory_page: self.recycled.memory_page.take(),
                            },
                            rewind_enabled: self.settings.rewind.enabled && !self.rewind.held,
                            rewind_seconds: self.settings.rewind.seconds,
                        };
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
                        self.fps_tracker.tick();
                    }
                }
            }
        }

        let should_render = match self.speed_mode() {
            SpeedMode::Normal => true,
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

        self.update_debug_cache_edges();

        if let Some(frame) = self.latest_frame.take() {
            if let Some(gfx) = self.gfx.as_mut() {
                if let Some((native_w, native_h)) =
                    native_size_for_frame(self.active_system, frame.len())
                {
                    gfx.set_native_size(native_w, native_h);
                } else {
                    log::warn!(
                        "Skipping frame upload with unexpected size: {} bytes for {:?}",
                        frame.len(),
                        self.active_system
                    );
                    self.recycled.framebuffer = Some(frame);
                    return;
                }
                gfx.upload_framebuffer(&frame);
            }
            self.last_displayed_frame = Some(frame.clone());
            self.recycled.framebuffer = Some(frame);
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
