use super::{App, SpeedMode};
use crate::debug::{ConsoleGraphicsData, DebugTab, is_tab_open};
use crate::emu_thread::{EmuResponse, FrameResult};
use std::time::Instant;

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

        if self.rewind.pending || self.rewind.backstep_pending {
            while let Some(resp) = self.emu_thread.as_ref().and_then(|t| t.try_recv_response()) {
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
            }
        }
    }

    pub(super) fn process_frame_result(&mut self, result: FrameResult) {
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);

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
}
