use super::{
    App, MAX_IN_FLIGHT, SETTINGS_UPDATE_INTERVAL, SpeedMode, UI_RENDER_INTERVAL,
    VIEWER_UPDATE_INTERVAL,
};
use crate::debug::{self, DebugTab, DebugUiActions, is_tab_open};
use crate::emu_thread::{
    AudioConfig, AudioRecordingCapture, EmuCommand, FrameInput, JoypadInput, SpeculationBlockers,
};
use crate::platform::Instant;

mod input;
mod nes_palette;
mod search;
mod snapshot;
mod timing;
mod zapper;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioSampleRateChange {
    Unchanged,
    Rejected,
    Applied,
}

fn reconcile_audio_sample_rate(
    last_sample_rate: u32,
    configured_sample_rate: &mut u32,
    recording_active: bool,
) -> AudioSampleRateChange {
    if last_sample_rate == *configured_sample_rate {
        return AudioSampleRateChange::Unchanged;
    }
    if recording_active {
        *configured_sample_rate = last_sample_rate;
        AudioSampleRateChange::Rejected
    } else {
        AudioSampleRateChange::Applied
    }
}

fn commit_pending_after_send(pending: &mut bool, sent: bool) {
    if sent {
        *pending = true;
    }
}

fn clear_dirty_after_send(dirty: &mut bool, sent: bool) {
    if sent {
        *dirty = false;
    }
}

fn commit_frame_send(frames_in_flight: &mut usize, sent: bool) -> bool {
    if sent {
        *frames_in_flight += 1;
    }
    sent
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

    pub(super) fn tick(&mut self) {
        #[cfg(target_arch = "wasm32")]
        self.poll_retired_wasm_threads();
        #[cfg(not(target_arch = "wasm32"))]
        if self.pce_mouse_captured
            && (!super::window_events::pce_mouse_capture_allowed(
                self.active_system,
                self.settings.emulation.pce_controller,
                self.rom_info.pce_controller_profile_hash,
            ) || self.settings.emulation.pce_mouse_cursor_mode
                != crate::settings::PceMouseCursorMode::Captured)
        {
            self.release_pce_mouse(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.fence_tas_control_gameplay();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let gameplay_commands_allowed = self.worker_gameplay_commands_allowed();
        #[cfg(target_arch = "wasm32")]
        let gameplay_commands_allowed = true;
        if gameplay_commands_allowed {
            self.sync_speed_setting();
        }
        match gameplay_commands_allowed.then(|| {
            reconcile_audio_sample_rate(
                self.last_audio_output_sample_rate,
                &mut self.settings.audio.output_sample_rate,
                self.recording.audio_recorder.is_some(),
            )
        }) {
            Some(AudioSampleRateChange::Rejected) => {
                self.toast_manager
                    .error("Stop audio recording before changing output sample rate");
            }
            Some(AudioSampleRateChange::Applied) => {
                self.last_audio_output_sample_rate = self.settings.audio.output_sample_rate;
                self.reset_audio_output();
                self.settings.save();
            }
            Some(AudioSampleRateChange::Unchanged) | None => {}
        }
        self.poll_gamepad();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_rom_preparation();

        let host_tilt = self.update_host_tilt_and_stick_mode();

        self.drain_emu_responses();
        #[cfg(not(target_arch = "wasm32"))]
        self.pump_pending_tas_repair_activation();
        #[cfg(not(target_arch = "wasm32"))]
        self.begin_queued_tas_control_acquire();
        #[cfg(not(target_arch = "wasm32"))]
        self.pump_realtime_tas_recording();
        #[cfg(not(target_arch = "wasm32"))]
        self.pump_linked_tas_playback();
        let supports_rewind = self.core_supports_rewind();
        let rewind_available = supports_rewind && !self.recording.is_replay_active();
        if !rewind_available {
            self.rewind.held = false;
        }
        let supports_audio = self.core_supports_audio();
        let supports_cheats = self.core_supports_cheats();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_symbol_load();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_replay_save_worker();

        if gameplay_commands_allowed
            && std::mem::take(&mut self.debug_requests.backstep)
            && rewind_available
            && self.settings.rewind.enabled
            && !self.rewind.pending
            && !self.rewind.backstep_pending
        {
            let sent = self.send_emu_command_checked(EmuCommand::Rewind(1)).is_ok();
            commit_pending_after_send(&mut self.rewind.backstep_pending, sent);
        }

        if gameplay_commands_allowed
            && self.rewind.held
            && rewind_available
            && self.settings.rewind.enabled
        {
            let now = Instant::now();
            if self.rewind.active_mode != Some(self.settings.rewind.mode) {
                self.rewind.reset_pacing();
                self.rewind.active_mode = Some(self.settings.rewind.mode);
            }
            if let Some(previous) = self.rewind.pace_updated_at.replace(now) {
                self.rewind.pacer.elapse(now.duration_since(previous));
            }
            if !self.rewind.pending && !self.rewind.backstep_pending {
                let steps = match self.settings.rewind.mode {
                    crate::settings::RewindMode::RealTime if self.rewind.pacer.ready() => 1,
                    crate::settings::RewindMode::RealTime => 0,
                    crate::settings::RewindMode::Fast => self.settings.rewind.speed.max(1),
                };
                if steps > 0
                    && self
                        .send_emu_command_checked(EmuCommand::Rewind(steps))
                        .is_ok()
                {
                    if self.settings.rewind.mode == crate::settings::RewindMode::RealTime {
                        let frames = self.settings.rewind.capture_interval() as u64;
                        self.rewind
                            .pacer
                            .schedule(self.nominal_frame_duration_ns(), frames);
                        self.rewind.scheduled_frames = frames;
                    }
                    commit_pending_after_send(&mut self.rewind.pending, true);
                }
            }
        } else {
            if self.rewind.pace_updated_at.is_some() {
                self.timing.last_frame_time = Instant::now();
            }
            self.rewind.reset_pacing();
            self.rewind.frames_rewound = 0;
            let max_in_flight = if self.recording.limits_in_flight_for_replay() {
                1
            } else {
                MAX_IN_FLIGHT
            };

            if gameplay_commands_allowed && self.frames_in_flight < max_in_flight {
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
                #[cfg(not(test))]
                let speculation_blockers = SpeculationBlockers::feature_disabled();
                #[cfg(test)]
                let speculation_blockers = {
                    #[cfg(not(target_arch = "wasm32"))]
                    let live_control_active = self.live_control.is_enabled();
                    #[cfg(target_arch = "wasm32")]
                    let live_control_active = false;
                    SpeculationBlockers::from_app_for_test(
                        self.recording.replay_timeline_active(),
                        live_control_active,
                    )
                };
                let debug_request_pending = self.debug_requests.has_pending();
                let debug_actions_pending = self.pending_debug_actions.has_pending();
                let has_pending =
                    debug_request_pending || debug_actions_pending || remote_capture_pending;
                #[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
                {
                    let perf_requirement = self.debug_workspace_visible()
                        && debug::compute_tab_requirements(&self.debug_dock).needs_perf_info;
                    super::browser_speculation_test::record_tick_request_state(
                        frames_to_step,
                        debug_request_pending,
                        debug_actions_pending,
                        remote_capture_pending,
                        is_tab_open(&self.debug_dock, DebugTab::Performance),
                        perf_requirement,
                    );
                }

                if (frames_to_step > 0 || has_pending)
                    && self
                        .preflight_emu_command_kind(
                            crate::emu_thread::TasControlCommandKind::FrameExecution,
                        )
                        .is_ok()
                {
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
                    let (buttons_pressed_p3, dpad_pressed_p3) =
                        replay_joypad_frames.as_ref().map_or_else(
                            || self.current_host_joypad_p3_input(),
                            |frames| {
                                frames
                                    .first()
                                    .map(|frame| (frame.buttons_p3, frame.dpad_p3))
                                    .unwrap_or((0, 0))
                            },
                        );
                    let (buttons_pressed_p4, dpad_pressed_p4) =
                        replay_joypad_frames.as_ref().map_or_else(
                            || self.current_host_joypad_p4_input(),
                            |frames| {
                                frames
                                    .first()
                                    .map(|frame| (frame.buttons_p4, frame.dpad_p4))
                                    .unwrap_or((0, 0))
                            },
                        );
                    let (buttons_pressed_p5, dpad_pressed_p5) =
                        replay_joypad_frames.as_ref().map_or_else(
                            || self.current_host_joypad_p5_input(),
                            |frames| {
                                frames
                                    .first()
                                    .map(|frame| (frame.buttons_p5, frame.dpad_p5))
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
                    let pce_mouse = self.pce_mouse_input(frames_to_step > 0);
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
                    let audio_playback_speed = match self.speed_mode() {
                        SpeedMode::FastForward => {
                            self.settings.emulation.fast_forward_multiplier.clamp(1, 16)
                        }
                        SpeedMode::Normal | SpeedMode::SlowMotion | SpeedMode::Uncapped => 1,
                    };

                    let input = FrameInput {
                        frames: frames_to_step,
                        speculation_blockers,
                        replay_joypad_frames,
                        host_tilt,
                        host_camera_frame,
                        joypad: JoypadInput {
                            buttons: buttons_pressed,
                            dpad: dpad_pressed,
                            buttons_p2: buttons_pressed_p2,
                            dpad_p2: dpad_pressed_p2,
                            buttons_p3: buttons_pressed_p3,
                            dpad_p3: dpad_pressed_p3,
                            buttons_p4: buttons_pressed_p4,
                            dpad_p4: dpad_pressed_p4,
                            buttons_p5: buttons_pressed_p5,
                            dpad_p5: dpad_pressed_p5,
                        },
                        pce_mouse,
                        zapper,
                        debug_step: std::mem::take(&mut self.debug_requests.step),
                        debug_continue: std::mem::take(&mut self.debug_requests.continue_)
                            || next_frame_requested,
                        debug_suspend_after_frame: next_frame_requested,
                        audio: AudioConfig {
                            apu_capture_enabled: supports_audio
                                && reqs.needs_apu
                                && want_viewer_update,
                            skip_audio: !supports_audio
                                || match self.speed_mode() {
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
                            playback_speed: audio_playback_speed,
                            recording_capture: AudioRecordingCapture {
                                active: supports_audio && self.recording.audio_recorder.is_some(),
                                semantic: supports_audio
                                    && self
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
                        rewind_enabled: supports_rewind
                            && self.settings.rewind.enabled
                            && !self.recording.is_replay_active()
                            && !self.rewind.held,
                        rewind_seconds: self.settings.rewind.seconds,
                    };

                    let mut command_send_allowed = true;
                    if self.debug_windows.cheat.cheats_dirty
                        && self.recording.allows_cheat_updates()
                        && supports_cheats
                    {
                        if self
                            .preflight_emu_command_kind(
                                crate::emu_thread::TasControlCommandKind::CheatConfiguration,
                            )
                            .is_err()
                        {
                            command_send_allowed = false;
                        } else {
                            let command =
                                EmuCommand::UpdateCheats(crate::cheats::collect_enabled_patches(
                                    &self.debug_windows.cheat.user_codes,
                                    &self.debug_windows.cheat.libretro_codes,
                                ));
                            let sent = self.send_emu_command_checked(command).is_ok();
                            clear_dirty_after_send(
                                &mut self.debug_windows.cheat.cheats_dirty,
                                sent,
                            );
                            if !sent {
                                command_send_allowed = false;
                            }
                        }
                    }
                    let frame_sent = command_send_allowed
                        && self
                            .send_emu_command_checked(EmuCommand::StepFrames(Box::new(input)))
                            .is_ok();
                    if commit_frame_send(&mut self.frames_in_flight, frame_sent) {
                        #[cfg(not(target_arch = "wasm32"))]
                        if frames_to_step > 0 {
                            self.advance_live_button_releases(frames_to_step);
                        }

                        if frames_to_step > 0 {
                            self.fps_tracker.tick_n(frames_to_step);
                        }
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
                            || self.show_mods_window
                            || self.show_cheats_window
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
        #[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
        super::browser_speculation_test::observe_app_tick(self, rendered);
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

        let mods_visible = self.show_mods_window
            && self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::mods_window)
                .is_some_and(|window| window.is_minimized() != Some(true));
        if mods_visible && now.duration_since(self.last_mods_render) >= SETTINGS_UPDATE_INTERVAL {
            self.render_mods_frame();
            self.last_mods_render = now;
        }

        let cheats_visible = self.show_cheats_window
            && self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::cheats_window)
                .is_some_and(|window| window.is_minimized() != Some(true));
        if cheats_visible && now.duration_since(self.last_cheats_render) >= SETTINGS_UPDATE_INTERVAL
        {
            self.render_cheats_frame();
            self.last_cheats_render = now;
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

        let tas_editor_visible = self.debug_windows.tas_editor.open
            && self.debug_windows.tas_editor.presentation()
                == crate::debug::TasEditorPresentation::SeparateWindow
            && self
                .gfx
                .as_ref()
                .and_then(crate::graphics::Graphics::tas_editor_window)
                .is_some_and(|window| window.is_minimized() != Some(true));
        if tas_editor_visible
            && now.duration_since(self.debug_windows.tas_editor.last_host_render())
                >= VIEWER_UPDATE_INTERVAL
        {
            self.render_tas_editor_frame();
            self.debug_windows.tas_editor.mark_host_rendered();
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::audio_recorder::AudioRecorder;
    use crate::settings::AudioRecordingFormat;

    #[test]
    fn speculation_blockers_require_an_inactive_replay_timeline_and_live_control() {
        assert!(SpeculationBlockers::feature_disabled().any());
        assert!(!SpeculationBlockers::from_app_for_test(false, false).any());
        assert!(SpeculationBlockers::from_app_for_test(true, false).any());
        assert!(SpeculationBlockers::from_app_for_test(false, true).any());
        assert!(SpeculationBlockers::from_app_for_test(true, true).any());
    }

    #[test]
    fn recording_rate_change_is_rejected_and_wav_metadata_stays_at_core_rate() {
        let core_rate = 48_000;
        let mut configured_rate = 96_000;
        assert_eq!(
            reconcile_audio_sample_rate(core_rate, &mut configured_rate, true),
            AudioSampleRateChange::Rejected
        );
        assert_eq!(configured_rate, core_rate);

        let path = std::env::temp_dir().join(format!(
            "zeff-audio-rate-policy-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder =
            AudioRecorder::start(&path, configured_rate, AudioRecordingFormat::Wav16, None)
                .unwrap();
        recorder.write_samples(&[0.0, 0.0]);
        recorder.finish().unwrap();

        let data = std::fs::read(&path).unwrap();
        assert_eq!(
            u32::from_le_bytes(data[24..28].try_into().unwrap()),
            core_rate
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn command_side_effects_commit_only_after_send() {
        let mut pending = false;
        let mut dirty = true;
        let mut in_flight = 2;

        commit_pending_after_send(&mut pending, false);
        clear_dirty_after_send(&mut dirty, false);
        assert!(!commit_frame_send(&mut in_flight, false));
        assert!(!pending);
        assert!(dirty);
        assert_eq!(in_flight, 2);

        commit_pending_after_send(&mut pending, true);
        clear_dirty_after_send(&mut dirty, true);
        assert!(commit_frame_send(&mut in_flight, true));
        assert!(pending);
        assert!(!dirty);
        assert_eq!(in_flight, 3);
    }
}
