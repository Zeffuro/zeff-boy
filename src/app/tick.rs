use super::{
    ActiveSystem, App, MAX_FRAMES_PER_TICK, MAX_IN_FLIGHT, SpeedMode, UI_RENDER_INTERVAL,
    VIEWER_UPDATE_INTERVAL,
};
use crate::debug::dock::TabDataRequirements;
use crate::debug::{self, DebugTab, DebugUiActions, is_tab_open};
use crate::emu_thread::{
    AudioConfig, EmuCommand, FrameInput, JoypadInput, MemorySearchRequest, RenderSettings,
    ReusableBuffers, SnapshotRequest, ZapperInput,
};
use crate::platform::Instant;
use crate::settings::{GamepadAction, NesPaletteMode};
use zeff_emu_common::system::{NES_SCREEN_SIZE, RGBA_BYTES_PER_PIXEL, rgba_framebuffer_len};

fn parse_pending_search(state: &mut impl SearchableState) -> Option<MemorySearchRequest> {
    if !state.search_pending() {
        return None;
    }
    state.set_search_pending(false);
    debug::hex_search::parse_search_query(state.search_query(), state.search_mode()).map(
        |pattern| MemorySearchRequest {
            pattern,
            max_results: state.search_max_results(),
        },
    )
}

trait SearchableState {
    fn search_pending(&self) -> bool;
    fn set_search_pending(&mut self, v: bool);
    fn search_query(&self) -> &str;
    fn search_mode(&self) -> crate::debug::MemorySearchMode;
    fn search_max_results(&self) -> usize;
}

impl SearchableState for crate::debug::MemoryViewerState {
    fn search_pending(&self) -> bool {
        self.search_pending
    }
    fn set_search_pending(&mut self, v: bool) {
        self.search_pending = v;
    }
    fn search_query(&self) -> &str {
        &self.search_query
    }
    fn search_mode(&self) -> crate::debug::MemorySearchMode {
        self.search_mode
    }
    fn search_max_results(&self) -> usize {
        self.search_max_results
    }
}

impl SearchableState for crate::debug::RomViewerState {
    fn search_pending(&self) -> bool {
        self.search_pending
    }
    fn set_search_pending(&mut self, v: bool) {
        self.search_pending = v;
    }
    fn search_query(&self) -> &str {
        &self.search_query
    }
    fn search_mode(&self) -> crate::debug::MemorySearchMode {
        self.search_mode
    }
    fn search_max_results(&self) -> usize {
        self.search_max_results
    }
}

struct NesPaletteSource {
    key: String,
    label: String,
    kind: NesPaletteSourceKind,
}

enum NesPaletteSourceKind {
    #[cfg(not(target_arch = "wasm32"))]
    Path(String),
    #[cfg(target_arch = "wasm32")]
    Bytes(Vec<u8>),
}

fn load_nes_palette_source(
    source: &NesPaletteSource,
) -> Result<zeff_nes_core::hardware::ppu::NesPalette, String> {
    let bytes = match source.kind {
        #[cfg(not(target_arch = "wasm32"))]
        NesPaletteSourceKind::Path(ref path) => {
            std::fs::read(path).map_err(|err| err.to_string())?
        }
        #[cfg(target_arch = "wasm32")]
        NesPaletteSourceKind::Bytes(ref bytes) => bytes.clone(),
    };
    zeff_nes_core::hardware::ppu::parse_nes_palette_bytes(&bytes).map_err(|err| err.to_string())
}

impl App {
    fn nes_zapper_input(&self) -> ZapperInput {
        if self.active_system != ActiveSystem::Nes {
            return ZapperInput::default();
        }

        if let Some(zapper) = self.remote_zapper {
            return zapper;
        }

        if !self.settings.emulation.nes_zapper_enabled {
            return ZapperInput::default();
        }

        ZapperInput {
            enabled: true,
            trigger: self.mouse_left_pressed && self.game_view_focused,
            hit: self.nes_zapper_hit(),
            screen_pos: self.nes_zapper_screen_pos(),
        }
    }

    fn nes_zapper_screen_pos(&self) -> Option<(u16, u16)> {
        let (cursor_x, cursor_y) = self.cursor_pos?;
        let gfx = self.gfx.as_ref()?;
        let (pixel_x, pixel_y) = gfx.game_pixel_at_window_pos(cursor_x, cursor_y)?;
        let (nes_width, nes_height) = NES_SCREEN_SIZE;
        if pixel_x < nes_width && pixel_y < nes_height {
            Some((pixel_x as u16, pixel_y as u16))
        } else {
            None
        }
    }

    fn nes_zapper_hit(&self) -> bool {
        let Some((pixel_x, pixel_y)) = self.nes_zapper_screen_pos() else {
            return false;
        };
        let Some(frame) = self.latest_frame.as_ref() else {
            return false;
        };
        if frame.len() != rgba_framebuffer_len(NES_SCREEN_SIZE) {
            return false;
        }

        const SAMPLE_RADIUS: i32 = 4;
        let (nes_width, nes_height) = NES_SCREEN_SIZE;
        let max_x = nes_width as i32 - 1;
        let max_y = nes_height as i32 - 1;
        let center_x = pixel_x as i32;
        let center_y = pixel_y as i32;

        for y in (center_y - SAMPLE_RADIUS).max(0)..=(center_y + SAMPLE_RADIUS).min(max_y) {
            for x in (center_x - SAMPLE_RADIUS).max(0)..=(center_x + SAMPLE_RADIUS).min(max_x) {
                let idx = ((y as usize * nes_width as usize + x as usize) * RGBA_BYTES_PER_PIXEL)
                    .min(frame.len() - RGBA_BYTES_PER_PIXEL);
                if Self::nes_zapper_pixel_is_bright(frame[idx], frame[idx + 1], frame[idx + 2]) {
                    return true;
                }
            }
        }

        false
    }

    fn nes_zapper_pixel_is_bright(r: u8, g: u8, b: u8) -> bool {
        let min_component = r.min(g).min(b);
        let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
        min_component >= 160 && luma >= 190.0
    }

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
            } else if let Some(button) = self.debug_windows.rebinding_ws_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set_ws(button, button_name);
                    self.debug_windows.rebinding_ws_gamepad = None;
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
                for (button, pressed) in poll.ws_events {
                    self.host_input.set_ws_gamepad(button, pressed);
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

            self.tilt.left_stick = poll.left_stick;
        }
    }

    pub(super) fn compute_frames_to_step(&mut self, now: Instant) -> usize {
        match self.speed_mode() {
            SpeedMode::Uncapped => {
                self.timing.last_frame_time = now;
                #[cfg(target_arch = "wasm32")]
                {
                    MAX_FRAMES_PER_TICK
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    1
                }
            }
            SpeedMode::Normal | SpeedMode::SlowMotion | SpeedMode::FastForward => {
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

                #[cfg(target_arch = "wasm32")]
                if matches!(self.speed_mode(), SpeedMode::Normal | SpeedMode::SlowMotion)
                    && frames > 3
                {
                    self.timing.last_frame_time = now;
                    frames = 3;
                }

                frames
            }
        }
    }

    fn gather_joypad_input(&mut self) -> (u8, u8) {
        let (mut buttons, dpad) = if let Some(player) = &mut self.recording.replay_player {
            if let Some((buttons, dpad)) = player.next_frame() {
                (buttons, dpad)
            } else {
                self.toast_manager.info("Replay finished");
                self.recording.replay_player = None;
                self.current_host_joypad_input()
            }
        } else {
            self.current_host_joypad_input()
        };

        if self.speed.turbo_held {
            self.speed.turbo_counter = self.speed.turbo_counter.wrapping_add(1);
            if self.speed.turbo_counter % 2 == 1 {
                buttons = 0;
            }
        } else {
            self.speed.turbo_counter = 0;
        }

        if let Some(recorder) = &mut self.recording.replay_recorder {
            recorder.record_frame(buttons, dpad);
        }

        (buttons, dpad)
    }

    fn build_snapshot_request(
        &mut self,
        reqs: &TabDataRequirements,
        want_viewer_update: bool,
    ) -> SnapshotRequest {
        let nes_custom_palette = self.nes_custom_palette_for_render();
        let remote_wants_debug = if self.remote_debug_frames_remaining > 0 {
            self.remote_debug_frames_remaining -= 1;
            true
        } else {
            false
        };
        let remote_wants_memory = if self.remote_memory_frames_remaining > 0 {
            self.remote_memory_frames_remaining -= 1;
            true
        } else {
            false
        };
        let remote_wants_graphics = if self.remote_graphics_frames_remaining > 0 {
            self.remote_graphics_frames_remaining -= 1;
            true
        } else {
            false
        };
        let memory_view_start = if remote_wants_memory {
            self.remote_memory_view_start
                .unwrap_or(self.debug_windows.memory.view_start)
        } else {
            self.debug_windows.memory.view_start
        };

        SnapshotRequest {
            want_debug_info: reqs.needs_debug_info || remote_wants_debug,
            want_perf_info: reqs.needs_perf_info || self.settings.ui.show_fps || remote_wants_debug,
            any_viewer_open: reqs.needs_viewer_data && want_viewer_update,
            any_vram_viewer_open: (reqs.needs_vram && want_viewer_update) || remote_wants_graphics,
            show_oam_viewer: (reqs.needs_oam && want_viewer_update) || remote_wants_graphics,
            show_apu_viewer: reqs.needs_apu && want_viewer_update,
            show_disassembler: reqs.needs_disassembly && want_viewer_update,
            show_rom_info: reqs.needs_rom_info && want_viewer_update,
            show_memory_viewer: (reqs.needs_memory_page && want_viewer_update)
                || remote_wants_memory,
            memory_view_start,
            show_rom_viewer: reqs.needs_rom_page && want_viewer_update,
            rom_view_start: self.debug_windows.rom_viewer.view_start,
            last_disasm_pc: self.debug_windows.last_disasm_pc,
            memory_search: parse_pending_search(&mut self.debug_windows.memory),
            rom_search: parse_pending_search(&mut self.debug_windows.rom_viewer),
            render: RenderSettings {
                color_correction: self.settings.video.gb_color_correction,
                color_correction_matrix: self.settings.video.gb_color_correction_matrix,
                dmg_palette_preset: self.settings.video.gb_dmg_palette_preset,
                nes_palette_mode: self.settings.video.nes_palette_mode,
                nes_custom_palette,
                sgb_border_enabled: self.settings.emulation.sgb_border_enabled,
            },
        }
    }

    fn nes_custom_palette_for_render(
        &mut self,
    ) -> Option<zeff_nes_core::hardware::ppu::NesPalette> {
        if self.settings.video.nes_palette_mode != NesPaletteMode::Custom {
            return None;
        }

        let Some(source) = self.nes_custom_palette_source() else {
            self.nes_palette_cache.path.clear();
            self.nes_palette_cache.palette = None;
            self.nes_palette_cache.error = None;
            return None;
        };

        if self.nes_palette_cache.path != source.key {
            self.nes_palette_cache.path = source.key.clone();
            match load_nes_palette_source(&source) {
                Ok(palette) => {
                    log::info!("Loaded NES palette from {}", source.label);
                    self.nes_palette_cache.palette = Some(palette);
                    self.nes_palette_cache.error = None;
                }
                Err(err) => {
                    log::warn!("Failed to load NES palette from {}: {err}", source.label);
                    self.nes_palette_cache.palette = None;
                    self.nes_palette_cache.error = Some(err);
                }
            }
        }

        self.nes_palette_cache.palette.clone()
    }

    fn nes_custom_palette_source(&self) -> Option<NesPaletteSource> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.settings.video.nes_custom_palette_path.trim();
            if path.is_empty() {
                None
            } else {
                Some(NesPaletteSource {
                    key: path.to_string(),
                    label: path.to_string(),
                    kind: NesPaletteSourceKind::Path(path.to_string()),
                })
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            if self.settings.video.nes_custom_palette_bytes.is_empty() {
                return None;
            }
            let name = if self.settings.video.nes_custom_palette_name.is_empty() {
                "uploaded .pal"
            } else {
                &self.settings.video.nes_custom_palette_name
            };
            return Some(NesPaletteSource {
                key: format!(
                    "embedded:{name}:{}",
                    self.settings.video.nes_custom_palette_bytes.len()
                ),
                label: name.to_string(),
                kind: NesPaletteSourceKind::Bytes(
                    self.settings.video.nes_custom_palette_bytes.clone(),
                ),
            });
        }
    }

    fn take_reusable_buffers(&mut self) -> ReusableBuffers {
        ReusableBuffers {
            audio: self.recycled.audio.take(),
            vram: self.recycled.vram.take(),
            oam: self.recycled.oam.take(),
            memory_page: self.recycled.memory_page.take(),
            nes_chr: self.recycled.nes_chr.take(),
            nes_nametable: self.recycled.nes_nametable.take(),
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
