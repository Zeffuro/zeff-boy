use super::{App, SpeedMode, MAX_FRAMES_PER_TICK, MAX_IN_FLIGHT, UI_RENDER_INTERVAL, VIEWER_UPDATE_INTERVAL};
use crate::debug::{self, DebugTab, DebugUiActions, ConsoleGraphicsData, MenuAction, is_tab_open};
use crate::emu_thread::{
    EmuCommand, EmuResponse, FrameInput, FrameResult, MemorySearchRequest, ReusableBuffers,
    SnapshotRequest,
};
use crate::graphics;
use crate::settings::GamepadAction;
use std::time::Instant;

impl App {
    pub(super) fn update_debug_cache_edges(&mut self) {
        if is_tab_open(&self.debug_dock, DebugTab::TileViewer) && !self.tile_viewer_was_open {
            self.debug_windows.tiles.invalidate_cache();
        }
        if is_tab_open(&self.debug_dock, DebugTab::TilemapViewer) && !self.tilemap_viewer_was_open {
            self.debug_windows.tilemap.invalidate_cache();
        }
    }

    pub(super) fn sync_speed_setting(&mut self) {
        if self.timing.uncapped_speed != self.settings.emulation.uncapped_speed {
            self.timing.uncapped_speed = self.settings.emulation.uncapped_speed;
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::SetUncapped(
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
                        GamepadAction::SpeedUp => {
                            self.fast_forward_held = pressed;
                        }
                        GamepadAction::Rewind => {
                            self.rewind.held = pressed;
                        }
                        GamepadAction::Pause => {
                            if pressed {
                                self.paused = !self.paused;
                                self.toast_manager.set_paused(self.paused);
                            }
                        }
                        GamepadAction::Turbo => {
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
                    EmuResponse::RewindOk { framebuffer } => {
                        self.latest_frame = Some(framebuffer);
                        if self.rewind.backstep_pending {
                            self.rewind.backstep_pending = false;
                            self.paused = true;
                            self.timing.last_frame_time = Instant::now();
                            self.toast_manager.set_paused(true);
                            self.toast_manager.info("⏮ Stepped back");
                        } else {
                            self.rewind.pending = false;
                            self.rewind.pops += 1;
                        }
                    }
                    EmuResponse::RewindFailed(msg) => {
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
            .and_then(|d| d.perf_info.as_ref())
            .map(|info| info.speed_mode_label.as_str());

        let is_recording = self.recording.audio_recorder.is_some();
        let is_recording_replay = self.recording.replay_recorder.is_some();
        let is_playing_replay = self.recording.replay_player.is_some();
        let is_rewinding = self.rewind.held && self.settings.rewind.enabled;
        let autohide_menu_bar = self.settings.ui.autohide_menu_bar;
        let cursor_y = self.cursor_pos.map(|(_, y)| y);
        let rewind_seconds_back =
            self.rewind.pops as f32 * self.settings.rewind.capture_interval() as f32 / 60.0;
        let slot_labels = super::state_io::build_slot_labels(self.cached_rom_hash, self.active_system);

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
            is_paused: self.paused,
            autohide_menu_bar,
            cursor_y,
            slot_labels,
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
                            self.paused = !self.paused;
                            self.toast_manager.set_paused(self.paused);
                        }
                        MenuAction::SpeedChange(delta) => {
                            let mult = self.settings.emulation.fast_forward_multiplier as i32 + delta;
                            self.settings.emulation.fast_forward_multiplier = mult.clamp(1, 16) as usize;
                            settings_dirty = true;
                        }
                        MenuAction::StartAudioRecording => self.start_audio_recording(),
                        MenuAction::StopAudioRecording => self.stop_audio_recording(),
                        MenuAction::StartReplayRecording => self.start_replay_recording(),
                        MenuAction::StopReplayRecording => self.stop_replay_recording(),
                        MenuAction::LoadReplay => self.load_and_play_replay(),
                        MenuAction::TakeScreenshot => self.take_screenshot(),
                        MenuAction::ToolbarSettingsChanged => settings_dirty = true,
                        MenuAction::SetLayerToggles(bg, win, sprites) => {
                            self.pending_debug_actions.layer_toggles = Some((*bg, *win, *sprites));
                        }
                        MenuAction::SetAspectRatio(_)
                        | MenuAction::OpenSettings => {}
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

        if self.settings.video.vsync_mode != self.timing.last_vsync_mode {
            self.timing.last_vsync_mode = self.settings.video.vsync_mode;
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_vsync(self.settings.video.vsync_mode);
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

    pub(super) fn process_frame_result(&mut self, result: FrameResult) {
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);
        if let Some(old) = self.latest_frame.replace(result.frame) {
            self.recycled.framebuffer = Some(old);
        }
        self.cached_is_mbc7 = result.is_mbc7;
        self.cached_is_pocket_camera = result.is_pocket_camera;
        self.rewind.fill = result.rewind_fill;

        if let Some(gamepad) = &mut self.gamepad {
            gamepad.set_rumble(result.rumble);
        }

        let fast_forward = matches!(self.speed_mode(), SpeedMode::FastForward);
        if let Some(audio) = &mut self.audio {
            audio.queue_samples(
                &result.audio_samples,
                self.settings.audio.volume,
                fast_forward,
                self.settings.audio.mute_during_fast_forward,
            );
        }

        if let Some(recorder) = &mut self.recording.audio_recorder {
            recorder.write_samples(&result.audio_samples);
            if let Some(snapshot) = result.apu_snapshot {
                recorder.write_apu_snapshot(snapshot);
            }
        }
        self.recycled.audio = Some(result.audio_samples);

        let mut ui_data = result.ui_data;

        if let Some(ref mut cached) = self.cached_ui_data {
            if ui_data.graphics_data.is_some() {
                if let Some(ConsoleGraphicsData::Gb(gb)) = cached.graphics_data.take()
                    && !gb.vram.is_empty()
                {
                    self.recycled.vram = Some(gb.vram);
                }
            } else {
                ui_data.graphics_data = cached.graphics_data.take();
            }
            if ui_data.oam_debug.is_none() {
                ui_data.oam_debug = cached.oam_debug.take();
            }
            if let Some(ref disasm) = ui_data.disassembly_view {
                self.debug_windows.last_disasm_pc = Some(disasm.pc);
            } else {
                ui_data.disassembly_view = cached.disassembly_view.take();
            }
            if ui_data.rom_debug.is_none() {
                ui_data.rom_debug = cached.rom_debug.take();
            }
            if ui_data.memory_page.is_some() {
                if let Some(old_page) = cached.memory_page.take() {
                    self.recycled.memory_page = Some(old_page);
                }
            } else {
                ui_data.memory_page = cached.memory_page.take();
            }
        }

        if let Some(ref mut perf) = ui_data.perf_info {
            perf.fps = if self.settings.ui.show_fps {
                self.fps_tracker.fps()
            } else {
                0.0
            };
            perf.speed_mode_label = self.speed_mode_label().to_string();
            perf.frames_in_flight = self.frames_in_flight;
        }

        if let Some(results) = ui_data.memory_search_results.take() {
            self.debug_windows.memory.search_results = results;
        }

        if let Some(results) = ui_data.rom_search_results.take() {
            self.debug_windows.rom_viewer.search_results = results;
        }
        self.debug_windows.rom_viewer.rom_size = ui_data.rom_size;

        match ui_data.graphics_data {
            Some(ConsoleGraphicsData::Gb(ref gb_data)) => {
                if is_tab_open(&self.debug_dock, DebugTab::TileViewer) {
                    self.debug_windows.tiles.update_dirty_inputs(gb_data);
                }
                if is_tab_open(&self.debug_dock, DebugTab::TilemapViewer) {
                    self.debug_windows.tilemap.update_dirty_inputs(gb_data);
                }
            }
            Some(ConsoleGraphicsData::Nes(_)) => {
                if is_tab_open(&self.debug_dock, DebugTab::TileViewer) {
                    self.debug_windows.tiles.invalidate_cache();
                }
                if is_tab_open(&self.debug_dock, DebugTab::TilemapViewer) {
                    self.debug_windows.tilemap.invalidate_cache();
                }
            }
            None => {}
        }

        self.cached_ui_data = Some(ui_data);
    }

    pub(super) fn tick(&mut self) {
        self.sync_speed_setting();
        self.poll_gamepad();

        let host_tilt = self.update_host_tilt_and_stick_mode();

        self.drain_emu_responses();

        // Handle backstep: pop one rewind snapshot and pause
        if std::mem::take(&mut self.debug_requests.backstep)
            && self.settings.rewind.enabled
            && !self.rewind.pending
            && !self.rewind.backstep_pending
            && let Some(thread) = &self.emu_thread {
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
                let frames_to_step = if self.paused {
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

                        if self.turbo_held {
                            self.turbo_counter = self.turbo_counter.wrapping_add(1);
                            if self.turbo_counter % 2 == 1 {
                                buttons_pressed = 0;
                            }
                        } else {
                            self.turbo_counter = 0;
                        }

                        if let Some(recorder) = &mut self.recording.replay_recorder {
                            recorder.record_frame(buttons_pressed, dpad_pressed);
                        }

                        let reqs = debug::compute_tab_requirements(&self.debug_dock);
                        let input = FrameInput {
                            frames: frames_to_step,
                            host_tilt,
                            host_camera_frame,
                            buttons_pressed,
                            dpad_pressed,
                            buttons_pressed_p2: 0,
                            dpad_pressed_p2: 0,
                            debug_step: std::mem::take(&mut self.debug_requests.step),
                            debug_continue: std::mem::take(&mut self.debug_requests.continue_),
                            apu_capture_enabled: reqs.needs_apu && want_viewer_update,
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
                            debug_actions: std::mem::replace(
                                &mut self.pending_debug_actions,
                                DebugUiActions::none(),
                            ),
                            snapshot: SnapshotRequest {
                                want_debug_info: (reqs.needs_debug_info || self.settings.ui.show_fps),
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
                                    debug::hex_viewer::parse_search_query(
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
                                    debug::hex_viewer::parse_search_query(
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
                                color_correction: self.settings.video.color_correction,
                                color_correction_matrix: self.settings.video.color_correction_matrix,
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

        self.tile_viewer_was_open = is_tab_open(&self.debug_dock, DebugTab::TileViewer);
        self.tilemap_viewer_was_open = is_tab_open(&self.debug_dock, DebugTab::TilemapViewer);
    }
}
