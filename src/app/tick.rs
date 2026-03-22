use super::{App, GB_FRAME_DURATION, SpeedMode, tilt::TiltFrameData};
use crate::{emu_thread::EmuResponse, graphics, hardware::types::CPUState, ui};
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
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_uncapped_present_mode(self.uncapped_speed);
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
            SpeedMode::FastForward => {
                self.last_frame_time = now;
                self.settings.fast_forward_multiplier
            }
            SpeedMode::Uncapped => {
                self.last_frame_time = now;
                self.settings.uncapped_frames_per_tick
            }
            SpeedMode::Normal => {
                let mut frames = 0;
                while self.last_frame_time + GB_FRAME_DURATION <= now {
                    frames += 1;
                    self.last_frame_time += GB_FRAME_DURATION;
                    if frames > 3 {
                        self.last_frame_time = now;
                        break;
                    }
                }
                frames
            }
        }
    }

    pub(super) fn apply_debug_run_control(&mut self) {
        let Some(emu) = &self.emulator else {
            return;
        };

        let mut emu = emu.lock().expect("emulator mutex poisoned");
        emu.bus.io.apu.debug_capture_enabled = self.debug_windows.show_apu_viewer;

        if matches!(emu.cpu.running, CPUState::Suspended) {
            if self.debug_continue_requested {
                emu.debug.clear_hits();
                emu.debug.break_on_next = false;
                emu.cpu.running = CPUState::Running;
                self.debug_continue_requested = false;
            } else if self.debug_step_requested {
                emu.debug.clear_hits();
                emu.debug.break_on_next = true;
                emu.cpu.running = CPUState::Running;
                self.debug_step_requested = false;
            }
        }
    }

    pub(super) fn drain_emu_responses(&mut self, fast_forward_active: bool) {
        let Some(thread) = &self.emu_thread else {
            return;
        };

        while let Some(response) = thread.try_recv() {
            match response {
                EmuResponse::FrameReady { frame, rumble } => {
                    self.latest_frame = Some(frame);
                    if let Some(gamepad) = &mut self.gamepad {
                        gamepad.set_rumble(rumble);
                    }
                }
                EmuResponse::AudioSamples(samples) => {
                    if let Some(audio) = &self.audio {
                        audio.queue_samples(
                            &samples,
                            self.settings.master_volume,
                            fast_forward_active,
                            self.settings.mute_audio_during_fast_forward,
                        );
                    }
                }
            }
        }
    }

    pub(super) fn build_ui_frame_data(
        &mut self,
        tilt_data: TiltFrameData,
    ) -> Option<ui::UiFrameData> {
        let Some(emu) = self.emulator.as_ref() else {
            return None;
        };

        let fps = self.fps_tracker.fps();
        let speed_mode_label = self.speed_mode_label();
        let emu = emu.lock().expect("emulator mutex poisoned");
        Some(ui::collect_ui_frame_data(
            &emu,
            &mut self.debug_windows,
            &self.settings,
            ui::UiTiltFrameData {
                is_mbc7: tilt_data.is_mbc7,
                stick_controls_tilt: tilt_data.stick_controls_tilt,
                keyboard: tilt_data.keyboard,
                mouse: tilt_data.mouse,
                left_stick: tilt_data.left_stick,
                target: tilt_data.target,
                smoothed: tilt_data.smoothed,
            },
            fps,
            speed_mode_label,
        ))
    }

    pub(super) fn render_frame(&mut self, ui_frame_data: Option<ui::UiFrameData>) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };

        let previous_settings = self.settings.clone();
        match gfx.render(
            ui_frame_data.as_ref().and_then(|d| d.debug_info.as_ref()),
            ui_frame_data.as_ref().and_then(|d| d.viewer_data.as_ref()),
            ui_frame_data
                .as_ref()
                .and_then(|d| d.rom_info_view.as_ref()),
            ui_frame_data
                .as_ref()
                .and_then(|d| d.disassembly_view.as_ref()),
            ui_frame_data
                .as_ref()
                .and_then(|d| d.memory_page.as_deref()),
            &mut self.debug_windows,
            &mut self.settings,
            &mut self.show_settings_window,
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
                ui::apply_debug_actions(
                    self.emulator.as_ref(),
                    &result.debug_actions,
                    &mut self.debug_step_requested,
                    &mut self.debug_continue_requested,
                );
                if !self.show_settings_window {
                    self.debug_windows.rebinding_action = None;
                }
            }
            Err(graphics::FrameError::Outdated) | Err(graphics::FrameError::Lost) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Timeout)
            | Err(graphics::FrameError::Occluded)
            | Err(graphics::FrameError::Validation) => {}
            Err(graphics::FrameError::OutOfMemory) => self.exit_requested = true,
        }

        if self.settings != previous_settings {
            self.settings.save();
        }

        true
    }
}
