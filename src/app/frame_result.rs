use super::media::fds_side_label;
use super::{App, SpeedMode};
use crate::debug::{ConsoleGraphicsData, DebugTab, RomInfoSection, is_tab_open};
use crate::emu_thread::{EmuResponse, FrameResult};
use crate::platform::Instant;

impl App {
    pub(super) fn drain_emu_responses(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.recording.is_replay_start_pending() {
            self.drain_emu_control_responses();
        }

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

        self.drain_emu_control_responses();
    }

    fn drain_emu_control_responses(&mut self) {
        while let Some(resp) = self.emu_thread.as_ref().and_then(|t| t.try_recv_response()) {
            if self.handle_link_response(&resp) {
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            let resp = match self.consume_replay_start_response(resp) {
                Some(resp) => resp,
                None => continue,
            };
            #[cfg(not(target_arch = "wasm32"))]
            let resp = match self.consume_replay_checkpoint_response(resp) {
                Some(resp) => resp,
                None => continue,
            };
            #[cfg(not(target_arch = "wasm32"))]
            let resp = match self.consume_replay_finalization_response(resp) {
                Some(resp) => resp,
                None => continue,
            };
            match resp {
                EmuResponse::GuestCallCompleted {
                    name,
                    instructions,
                    undo_state,
                } => {
                    self.debug_windows.console.guest_call_completed(
                        &name,
                        instructions,
                        undo_state,
                    );
                    self.debug_windows.last_disasm_pc = None;
                    self.debug_windows.last_disasm_mapping = None;
                    if let Some(thread) = &self.emu_thread {
                        self.latest_frame = thread.shared_framebuffer().load_full();
                    }
                    self.toast_manager
                        .success(format!("{name} returned ({instructions} instructions)"));
                    continue;
                }
                EmuResponse::GuestCallFailed { name, error } => {
                    self.debug_windows.console.guest_call_failed(&name, &error);
                    self.toast_manager.error(format!("{name}: {error}"));
                    continue;
                }
                EmuResponse::GuestCallUndone => {
                    self.debug_windows.console.guest_call_undone();
                    self.debug_windows.last_disasm_pc = None;
                    self.debug_windows.last_disasm_mapping = None;
                    if let Some(thread) = &self.emu_thread {
                        self.latest_frame = thread.shared_framebuffer().load_full();
                    }
                    self.toast_manager.success("Guest call undone");
                    continue;
                }
                EmuResponse::GuestCallUndoFailed(error) => {
                    self.debug_windows.console.guest_call_undo_failed(&error);
                    self.toast_manager
                        .error(format!("Could not undo call: {error}"));
                    continue;
                }
                _ => {}
            }
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
        #[cfg(not(target_arch = "wasm32"))]
        self.schedule_replay_checkpoint();
        if let Some(error) = result.replay_error.take() {
            log::warn!("Replay stopped: {error}");
            self.recording.replay_player = None;
            self.recording.pending_replay_batches.clear();
            self.recording.queued_replay_playback_frames = 0;
            self.resume_uncapped_worker_after_replay();
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
            for reason in result.audio_timeline_discontinuities {
                recorder.begin_semantic_timeline_epoch(reason);
            }
            for frame in result.audio_semantic_frames {
                recorder.write_audio_semantic_frame(frame);
            }
        }
        let mut reusable_audio = result.audio_samples;
        reusable_audio.clear();
        self.recycled.audio = Some(reusable_audio);

        let mut ui_data = result.ui_data;
        if let Some(batch) = ui_data.instruction_trace.take() {
            self.debug_windows
                .execution_coverage
                .merge(&batch, &self.symbols);
            self.debug_windows.trace.merge(batch);
        }

        if let Some(ref mut cached) = self.cached_ui_data {
            if ui_data.cpu_debug.is_none() {
                ui_data.cpu_debug = cached.cpu_debug.take();
            }
            if ui_data.perf_info.is_none() {
                ui_data.perf_info = cached.perf_info.take();
            }
            if ui_data.input_debug.is_none() {
                ui_data.input_debug = cached.input_debug.take();
            }
            if ui_data.palette_debug.is_none() {
                ui_data.palette_debug = cached.palette_debug.take();
            }
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
            if ui_data.disassembly_view.is_none() {
                ui_data.disassembly_view = cached.disassembly_view.take();
            }
            if ui_data.rom_debug.is_none() {
                ui_data.rom_debug = cached.rom_debug.take();
            }
            if ui_data.rom_page.is_none() {
                ui_data.rom_page = cached.rom_page.take();
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

        if let Some(ref mut disasm) = ui_data.disassembly_view {
            self.symbols.annotate_disassembly(disasm);
            self.debug_windows.last_disasm_pc = Some(disasm.pc);
            self.debug_windows.last_disasm_mapping = disasm.mapping;
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

        if let Some(ref mut info) = ui_data.rom_debug
            && !info
                .sections
                .iter()
                .any(|section| section.heading == "Symbols")
            && let Some(fields) = self.symbols.summary_fields()
        {
            info.sections.push(RomInfoSection {
                heading: "Symbols",
                fields,
            });
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
        let origin = self.recording.replay_recording_origin;
        let Some(recorder) = self.recording.replay_recorder_for_commits() else {
            return;
        };
        for event in events {
            let event = match Self::replay_relative_event(event, origin) {
                Ok(event) => event,
                Err(err) => {
                    log::warn!("Dropping replay event with invalid capture origin: {err}");
                    continue;
                }
            };
            recorder.record_event(event);
        }
    }

    fn replay_relative_event(
        event: zeff_emu_common::replay::ReplayEvent,
        origin: crate::app::types::ReplayCaptureOrigin,
    ) -> anyhow::Result<zeff_emu_common::replay::ReplayEvent> {
        match event {
            zeff_emu_common::replay::ReplayEvent::GameBoyLink { frame, tick, event } => {
                let relative_frame = frame.checked_sub(origin.frame).ok_or_else(|| {
                    anyhow::anyhow!(
                        "GB link event frame {frame} is before replay origin frame {}",
                        origin.frame
                    )
                })?;
                let relative_tick = if let Some(base_tick) = origin.game_boy_tick {
                    tick.checked_sub(base_tick).ok_or_else(|| {
                        anyhow::anyhow!(
                            "GB link event tick {tick} is before replay origin tick {base_tick}"
                        )
                    })?
                } else {
                    tick
                };
                Ok(zeff_emu_common::replay::ReplayEvent::GameBoyLink {
                    frame: relative_frame,
                    tick: relative_tick,
                    event,
                })
            }
            zeff_emu_common::replay::ReplayEvent::GameBoyLinkState { frame, state } => {
                let relative_frame = frame.checked_sub(origin.frame).ok_or_else(|| {
                    anyhow::anyhow!(
                        "GB link state event frame {frame} is before replay origin frame {}",
                        origin.frame
                    )
                })?;
                Ok(zeff_emu_common::replay::ReplayEvent::GameBoyLinkState {
                    frame: relative_frame,
                    state,
                })
            }
            zeff_emu_common::replay::ReplayEvent::WonderSwanLink {
                frame,
                session_cycle,
                event,
            } => {
                let relative_frame = frame.checked_sub(origin.frame).ok_or_else(|| {
                    anyhow::anyhow!(
                        "WonderSwan link event frame {frame} is before replay origin frame {}",
                        origin.frame
                    )
                })?;
                let relative_tick = if let Some(base_tick) = origin.wonder_swan_tick {
                    session_cycle.checked_sub(base_tick).ok_or_else(|| {
                        anyhow::anyhow!(
                            "WonderSwan link event tick {session_cycle} is before replay origin tick {base_tick}"
                        )
                    })?
                } else {
                    session_cycle
                };
                Ok(zeff_emu_common::replay::ReplayEvent::WonderSwanLink {
                    frame: relative_frame,
                    session_cycle: relative_tick,
                    event,
                })
            }
            event => Ok(event),
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
                        #[cfg(not(target_arch = "wasm32"))]
                        let frame = u64::try_from(player.cursor()).unwrap_or(u64::MAX);
                        #[cfg(not(target_arch = "wasm32"))]
                        let has_checkpoint = player
                            .metadata()
                            .checkpoints
                            .iter()
                            .any(|checkpoint| checkpoint.frame == frame);
                        #[cfg(target_arch = "wasm32")]
                        let has_checkpoint = false;
                        if !has_checkpoint {
                            self.toast_manager.info("Replay finished");
                            self.recording.replay_player = None;
                            self.resume_uncapped_worker_after_replay();
                        }
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

#[cfg(test)]
mod tests {
    use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent};

    use crate::app::types::ReplayCaptureOrigin;

    use super::App;

    #[test]
    fn replay_relative_event_rejects_pre_origin_frame() {
        let err = App::replay_relative_event(
            ReplayEvent::GameBoyLink {
                frame: 9,
                tick: 200,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 1,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 2,
                },
            },
            ReplayCaptureOrigin {
                frame: 10,
                game_boy_tick: Some(100),
                wonder_swan_tick: None,
            },
        )
        .expect_err("pre-origin event should be rejected");

        assert!(
            err.to_string().contains("before replay origin frame"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replay_relative_event_rejects_pre_origin_game_boy_tick() {
        let err = App::replay_relative_event(
            ReplayEvent::GameBoyLink {
                frame: 10,
                tick: 99,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 1,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 2,
                },
            },
            ReplayCaptureOrigin {
                frame: 10,
                game_boy_tick: Some(100),
                wonder_swan_tick: None,
            },
        )
        .expect_err("pre-origin tick should be rejected");

        assert!(
            err.to_string().contains("before replay origin tick"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replay_relative_event_rebases_frame_and_game_boy_tick() {
        let event = App::replay_relative_event(
            ReplayEvent::GameBoyLink {
                frame: 12,
                tick: 150,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 1,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 2,
                },
            },
            ReplayCaptureOrigin {
                frame: 10,
                game_boy_tick: Some(100),
                wonder_swan_tick: None,
            },
        )
        .expect("post-origin event should be valid");

        assert_eq!(
            event,
            ReplayEvent::GameBoyLink {
                frame: 2,
                tick: 50,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 1,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 2,
                },
            }
        );
    }
}
