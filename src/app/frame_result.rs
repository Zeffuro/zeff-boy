use super::media::fds_side_label;
use super::{App, SpeedMode};
use crate::debug::{ConsoleGraphicsData, DebugTab, is_tab_open};
use crate::emu_thread::{EmuResponse, FrameResult};
use crate::platform::Instant;

impl App {
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

        while let Some(resp) = self.emu_thread.as_ref().and_then(|t| t.try_recv_response()) {
            if self.handle_link_response(&resp) {
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            let resp = match self.consume_replay_finalization_response(resp) {
                Some(resp) => resp,
                None => continue,
            };
            match resp {
                EmuResponse::FdsDiskSideChanged(side) => {
                    if self.recording.replay_media_events_pending > 0 {
                        self.recording.replay_media_events_pending -= 1;
                        log::debug!("Replay selected FDS side {}", fds_side_label(side));
                        continue;
                    }
                    self.toast_manager
                        .success(format!("FDS side {} selected", fds_side_label(side)));
                    continue;
                }
                EmuResponse::FdsDiskSideChangeFailed(message) => {
                    if self.recording.replay_media_events_pending > 0 {
                        self.recording.replay_media_events_pending -= 1;
                        self.recording.replay_player = None;
                    }
                    self.toast_manager
                        .error(format!("FDS side change failed: {message}"));
                    continue;
                }
                _ => {}
            }
            if self.rewind.pending || self.rewind.backstep_pending {
                match resp {
                    EmuResponse::RewindOk => {
                        if let Some(thread) = &self.emu_thread {
                            self.latest_frame = thread.shared_framebuffer().load_full();
                        }
                        if self.rewind.backstep_pending {
                            self.rewind.backstep_pending = false;
                            self.speed.paused = true;
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
            } else {
                log::debug!("Ignoring unexpected emulator response while idle");
            }
        }
    }

    pub(super) fn process_frame_result(&mut self, mut result: FrameResult) {
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);
        self.record_replay_events(result.replay_events);
        self.commit_replay_batch(result.advanced_frames);
        if let Some(error) = result.replay_error.take() {
            log::warn!("Replay stopped: {error}");
            self.recording.replay_player = None;
            self.recording.pending_replay_batches.clear();
            self.recording.queued_replay_playback_frames = 0;
            self.toast_manager.error(format!("Replay stopped: {error}"));
        }

        // Read the latest framebuffer from the lock-free shared buffer
        if let Some(thread) = &self.emu_thread {
            self.latest_frame = thread.shared_framebuffer().load_full();
        }

        self.rom_info.is_mbc7 = result.is_mbc7;
        self.rom_info.is_pocket_camera = result.is_pocket_camera;
        self.rewind.fill = result.rewind_fill;

        if let Some(gamepad) = &mut self.gamepad {
            gamepad.set_rumble(result.rumble);
        }

        let fast_forward = matches!(self.speed_mode(), SpeedMode::FastForward);
        if let Some(audio) = &mut self.audio {
            audio.queue_samples(
                &result.audio_samples,
                &crate::audio::AudioQueueConfig {
                    master_volume: self.settings.audio.volume,
                    fast_forward_active: fast_forward,
                    mute_during_fast_forward: self.settings.audio.mute_during_fast_forward,
                    low_pass_enabled: self.settings.audio.low_pass_enabled,
                    low_pass_cutoff_hz: self.settings.audio.low_pass_cutoff_hz,
                },
            );
        }

        if let Some(recorder) = &mut self.recording.audio_recorder {
            recorder.write_samples(&result.audio_samples);
            for frame in result.audio_semantic_frames {
                recorder.write_audio_semantic_frame(frame);
            }
        }
        let mut reusable_audio = result.audio_samples;
        reusable_audio.clear();
        self.recycled.audio = Some(reusable_audio);

        let mut ui_data = result.ui_data;

        if let Some(ref mut cached) = self.cached_ui_data {
            if ui_data.graphics_data.is_some() {
                match cached.graphics_data.take() {
                    Some(ConsoleGraphicsData::Gb(gb)) if !gb.vram.is_empty() => {
                        self.recycled.vram = Some(gb.vram);
                        if !gb.oam.is_empty() {
                            self.recycled.oam = Some(gb.oam);
                        }
                    }
                    Some(ConsoleGraphicsData::Nes(nes)) => {
                        if !nes.chr_data.is_empty() {
                            self.recycled.nes_chr = Some(nes.chr_data);
                        }
                        if !nes.nametable_data.is_empty() {
                            self.recycled.nes_nametable = Some(nes.nametable_data);
                        }
                    }
                    Some(ConsoleGraphicsData::Sega8(sega8)) => {
                        if !sega8.vram.is_empty() {
                            self.recycled.vram = Some(sega8.vram);
                        }
                        if !sega8.oam.is_empty() {
                            self.recycled.oam = Some(sega8.oam);
                        }
                    }
                    _ => {}
                }
            } else {
                ui_data.graphics_data = cached.graphics_data.take();
            }
            if ui_data.oam_debug.is_none() {
                ui_data.oam_debug = cached.oam_debug.take();
            }
            if ui_data.apu_debug.is_none() {
                ui_data.apu_debug = cached.apu_debug.take();
            }
            if let Some(ref disasm) = ui_data.disassembly_view {
                self.debug_windows.last_disasm_pc = Some(disasm.pc);
            } else {
                ui_data.disassembly_view = cached.disassembly_view.take();
            }
            if ui_data.rom_debug.is_none() {
                ui_data.rom_debug = cached.rom_debug.take();
            }
            match ui_data.memory_page.take() {
                Some(page) if !page.is_empty() => {
                    if let Some(old_page) = cached.memory_page.take() {
                        self.recycled.memory_page = Some(old_page);
                    }
                    ui_data.memory_page = Some(page);
                }
                Some(empty_page) => {
                    self.recycled.memory_page = Some(empty_page);
                    ui_data.memory_page = cached.memory_page.take();
                }
                None => {
                    ui_data.memory_page = cached.memory_page.take();
                }
            }
        }

        if let Some(ref mut perf) = ui_data.perf_info {
            perf.fps = if self.settings.ui.show_fps {
                self.fps_tracker.fps()
            } else {
                0.0
            };
            perf.speed_mode_label = self.speed_mode_label();
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
            Some(ConsoleGraphicsData::Gba(_))
            | Some(ConsoleGraphicsData::Nes(_))
            | Some(ConsoleGraphicsData::Sega8(_)) => {
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

    fn record_replay_events(&mut self, events: Vec<zeff_emu_common::replay::ReplayEvent>) {
        let base_frame = self.recording.replay_recording_base_frame;
        let base_game_boy_tick = self.recording.replay_recording_base_game_boy_tick;
        let Some(recorder) = self.recording.replay_recorder_for_commits() else {
            return;
        };
        for event in events {
            recorder.record_event(Self::replay_relative_event(
                event,
                base_frame,
                base_game_boy_tick,
            ));
        }
    }

    fn replay_relative_event(
        event: zeff_emu_common::replay::ReplayEvent,
        base_frame: u64,
        base_game_boy_tick: Option<u64>,
    ) -> zeff_emu_common::replay::ReplayEvent {
        match event {
            zeff_emu_common::replay::ReplayEvent::GameBoyLink { frame, tick, event } => {
                zeff_emu_common::replay::ReplayEvent::GameBoyLink {
                    frame: frame.saturating_sub(base_frame),
                    tick: base_game_boy_tick
                        .map(|base_tick| tick.saturating_sub(base_tick))
                        .unwrap_or(tick),
                    event,
                }
            }
            zeff_emu_common::replay::ReplayEvent::GameBoyLinkState { frame, state } => {
                zeff_emu_common::replay::ReplayEvent::GameBoyLinkState {
                    frame: frame.saturating_sub(base_frame),
                    state,
                }
            }
            zeff_emu_common::replay::ReplayEvent::WonderSwanLink {
                frame,
                session_cycle,
                event,
            } => zeff_emu_common::replay::ReplayEvent::WonderSwanLink {
                frame: frame.saturating_sub(base_frame),
                session_cycle,
                event,
            },
            event => event,
        }
    }

    fn commit_replay_batch(&mut self, mut advanced_frames: usize) {
        while advanced_frames > 0 {
            let Some(mut batch) = self.recording.pending_replay_batches.pop_front() else {
                break;
            };

            let commit_count = advanced_frames.min(batch.frames.len());
            if batch.record
                && let Some(recorder) = self.recording.replay_recorder_for_commits()
            {
                for frame in batch.frames.iter().take(commit_count) {
                    recorder.record_joypad_frame(frame.clone());
                }
            }
            if batch.playback {
                if let Some(player) = &mut self.recording.replay_player {
                    player.advance_frames(commit_count);
                    if player.is_finished() {
                        self.toast_manager.info("Replay finished");
                        self.recording.replay_player = None;
                    }
                }
                self.recording.queued_replay_playback_frames = self
                    .recording
                    .queued_replay_playback_frames
                    .saturating_sub(commit_count);
            }

            advanced_frames -= commit_count;
            if commit_count < batch.frames.len() {
                batch.frames.drain(..commit_count);
                self.recording.pending_replay_batches.push_front(batch);
                break;
            }
        }
    }
}
