use anyhow::Result;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

use crate::{
    audio::AudioOutput,
    debug::{DebugUiActions, DebugWindowState, FpsTracker, ToastManager, create_default_dock_state, create_dock_from_saved_tabs},
    emu_thread::EmuThread,
    emulator::Emulator,
    graphics::Graphics,
    input::GamepadHandler,
    settings::{LeftStickMode, Settings},
    ui,
};

mod bindings;
mod host_sync;
mod input;
mod keyboard;
mod lifecycle;
mod shutdown;
mod state_io;
mod tick;
mod tilt;
mod window_events;

use input::HostInputState;
use tilt::{AutoTiltSource, TiltFrameData};

pub(crate) fn run(emulator: Option<Emulator>, settings: Settings) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let uncapped_speed = settings.uncapped_speed;

    // Cache metadata before handing emulator to emu thread
    let cached_is_mbc7 = emulator.as_ref().map_or(false, |e| e.is_mbc7_cartridge());
    let cached_rom_path = emulator.as_ref().map(|e| e.rom_path().to_path_buf());

    let mut app = App {
        emu_thread: None,
        initial_emulator: emulator,
        audio: None,
        gamepad: GamepadHandler::new(),
        gfx: None,
        window_id: None,
        fps_tracker: FpsTracker::new(),
        debug_windows: DebugWindowState::new(),
        debug_dock: if settings.open_debug_tabs.is_empty() {
            create_default_dock_state()
        } else {
            create_dock_from_saved_tabs(&settings.open_debug_tabs)
        },
        exit_requested: false,
        settings,
        last_frame_time: Instant::now(),
        last_render_time: Instant::now(),
        last_viewer_update: Instant::now(),
        uncapped_speed,
        fast_forward_held: false,
        shift_held: false,
        host_input: HostInputState::new(),
        cursor_pos: None,
        window_size: (160.0, 144.0),
        smoothed_tilt: (0.0, 0.0),
        left_stick: (0.0, 0.0),
        auto_tilt_source: AutoTiltSource::Keyboard,
        last_state_dir: None,
        show_settings_window: false,
        debug_step_requested: false,
        debug_continue_requested: false,
        latest_frame: None,
        recycled_framebuffer: None,
        recycled_audio_buffer: None,
        recycled_vram_buffer: None,
        recycled_oam_buffer: None,
        recycled_memory_page: None,
        frames_in_flight: 0,
        cached_ui_data: None,
        cached_is_mbc7,
        cached_rom_path,
        pending_debug_actions: DebugUiActions::none(),
        tile_viewer_was_open: false,
        tilemap_viewer_was_open: false,
        shutdown_performed: false,
        toast_manager: ToastManager::new(),
        audio_recorder: None,
        replay_recorder: None,
        replay_player: None,
        rewind_held: false,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    initial_emulator: Option<Emulator>,
    emu_thread: Option<EmuThread>,
    audio: Option<AudioOutput>,
    gamepad: Option<GamepadHandler>,
    gfx: Option<Graphics>,
    window_id: Option<WindowId>,
    fps_tracker: FpsTracker,
    debug_windows: DebugWindowState,
    debug_dock: egui_dock::DockState<crate::debug::DebugTab>,
    exit_requested: bool,
    settings: Settings,
    last_frame_time: Instant,
    last_render_time: Instant,
    last_viewer_update: Instant,
    uncapped_speed: bool,
    fast_forward_held: bool,
    shift_held: bool,
    host_input: HostInputState,
    cursor_pos: Option<(f32, f32)>,
    window_size: (f32, f32),
    smoothed_tilt: (f32, f32),
    left_stick: (f32, f32),
    auto_tilt_source: AutoTiltSource,
    last_state_dir: Option<PathBuf>,
    show_settings_window: bool,
    debug_step_requested: bool,
    debug_continue_requested: bool,
    latest_frame: Option<Vec<u8>>,

    recycled_framebuffer: Option<Vec<u8>>,
    recycled_audio_buffer: Option<Vec<f32>>,
    recycled_vram_buffer: Option<Vec<u8>>,
    recycled_oam_buffer: Option<Vec<u8>>,
    recycled_memory_page: Option<Vec<(u16, u8)>>,
    frames_in_flight: usize,
    cached_ui_data: Option<ui::UiFrameData>,
    cached_is_mbc7: bool,
    cached_rom_path: Option<PathBuf>,

    pending_debug_actions: DebugUiActions,
    tile_viewer_was_open: bool,
    tilemap_viewer_was_open: bool,
    shutdown_performed: bool,
    toast_manager: ToastManager,
    audio_recorder: Option<crate::audio_recorder::AudioRecorder>,
    replay_recorder: Option<crate::replay::ReplayRecorder>,
    replay_player: Option<crate::replay::ReplayPlayer>,
    rewind_held: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeedMode {
    Normal,
    Uncapped,
    FastForward,
}

const GB_FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);

const MAX_IN_FLIGHT: usize = 2;
const MAX_FRAMES_PER_TICK: usize = 10;

const UI_RENDER_INTERVAL: Duration = Duration::from_millis(16);

const VIEWER_UPDATE_INTERVAL: Duration = Duration::from_millis(33); // ~30Hz

impl App {

    fn speed_mode(&self) -> SpeedMode {
        if self.uncapped_speed {
            SpeedMode::Uncapped
        } else if self.fast_forward_held {
            SpeedMode::FastForward
        } else {
            SpeedMode::Normal
        }
    }

    fn speed_mode_label(&self) -> &'static str {
        match self.speed_mode() {
            SpeedMode::Normal => "Normal",
            SpeedMode::Uncapped => "Uncapped (Benchmark)",
            SpeedMode::FastForward => "Fast",
        }
    }

    fn effective_frame_duration(&self) -> Duration {
        match self.speed_mode() {
            SpeedMode::FastForward => {
                let mult = self.settings.fast_forward_multiplier.max(1) as u32;
                GB_FRAME_DURATION / mult
            }
            _ => GB_FRAME_DURATION,
        }
    }

    fn left_stick_controls_tilt(&self, is_mbc7: bool) -> bool {
        match self.settings.left_stick_mode {
            LeftStickMode::Tilt => true,
            LeftStickMode::Dpad => false,
            LeftStickMode::Auto => is_mbc7,
        }
    }

    fn left_stick_controls_dpad(&self, is_mbc7: bool) -> bool {
        !self.left_stick_controls_tilt(is_mbc7)
    }

    fn mouse_tilt_vector(&self) -> (f32, f32) {
        tilt::mouse_tilt_vector(self.cursor_pos, self.window_size)
    }

    fn compute_target_tilt(
        &mut self,
        is_mbc7: bool,
        keyboard: (f32, f32),
        mouse: (f32, f32),
        left_stick: (f32, f32),
    ) -> (f32, f32) {
        let stick_controls_tilt = self.left_stick_controls_tilt(is_mbc7);
        tilt::compute_target_tilt(
            is_mbc7,
            self.settings.tilt_input_mode,
            &mut self.auto_tilt_source,
            keyboard,
            mouse,
            left_stick,
            stick_controls_tilt,
            self.settings.tilt_sensitivity,
            self.settings.tilt_invert_x,
            self.settings.tilt_invert_y,
        )
    }

    fn update_smoothed_tilt(&mut self, target: (f32, f32), is_mbc7: bool) -> (f32, f32) {
        let stick_controls_tilt = self.left_stick_controls_tilt(is_mbc7);
        tilt::update_smoothed_tilt(
            &mut self.smoothed_tilt,
            target,
            is_mbc7,
            self.left_stick,
            stick_controls_tilt,
            self.settings.tilt_deadzone,
            self.settings.stick_tilt_bypass_lerp,
            self.settings.tilt_lerp,
        )
    }

    fn update_host_tilt_and_stick_mode(&mut self) -> TiltFrameData {
        let is_mbc7 = self.cached_is_mbc7;
        let keyboard = self.host_input.tilt_vector();
        let mouse = self.mouse_tilt_vector();
        let left_stick = self.left_stick;

        self.sync_host_input_with_stick_mode(is_mbc7);
        let target = self.compute_target_tilt(is_mbc7, keyboard, mouse, left_stick);
        let smoothed = self.update_smoothed_tilt(target, is_mbc7);

        TiltFrameData {
            is_mbc7,
            stick_controls_tilt: self.left_stick_controls_tilt(is_mbc7),
            keyboard,
            mouse,
            left_stick,
            target,
            smoothed,
        }
    }

    fn recv_cold_response(&mut self) -> Option<crate::emu_thread::EmuResponse> {
        while let Some(result) = self.emu_thread.as_ref()?.try_recv_frame() {
            self.process_frame_result(result);
        }
        self.emu_thread.as_ref()?.recv()
    }

    fn process_frame_result(&mut self, result: crate::emu_thread::FrameResult) {
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);
        if let Some(old) = self.latest_frame.replace(result.frame) {
            self.recycled_framebuffer = Some(old);
        }
        self.cached_is_mbc7 = result.is_mbc7;

        if let Some(gamepad) = &mut self.gamepad {
            gamepad.set_rumble(result.rumble);
        }

        let fast_forward = matches!(self.speed_mode(), SpeedMode::FastForward);
        if let Some(audio) = &mut self.audio {
            audio.queue_samples(
                &result.audio_samples,
                self.settings.master_volume,
                fast_forward,
                self.settings.mute_audio_during_fast_forward,
            );
        }

        if let Some(recorder) = &mut self.audio_recorder {
            recorder.write_samples(&result.audio_samples);
        }
        self.recycled_audio_buffer = Some(result.audio_samples);

        let mut ui_data = result.ui_data;

        if let Some(ref mut cached) = self.cached_ui_data {
            if ui_data.viewer_data.is_some() {
                if let Some(old_viewer) = cached.viewer_data.take() {
                    if !old_viewer.vram.is_empty() {
                        self.recycled_vram_buffer = Some(old_viewer.vram);
                    }
                    if !old_viewer.oam.is_empty() {
                        self.recycled_oam_buffer = Some(old_viewer.oam);
                    }
                }
            } else {
                ui_data.viewer_data = cached.viewer_data.take();
            }
            if let Some(ref disasm) = ui_data.disassembly_view {
                self.debug_windows.last_disasm_pc = Some(disasm.pc);
            } else {
                ui_data.disassembly_view = cached.disassembly_view.take();
            }
            if ui_data.rom_info_view.is_none() {
                ui_data.rom_info_view = cached.rom_info_view.take();
            }
            if ui_data.memory_page.is_some() {
                if let Some(old_page) = cached.memory_page.take() {
                    self.recycled_memory_page = Some(old_page);
                }
            } else {
                ui_data.memory_page = cached.memory_page.take();
            }
        }

        if let Some(ref mut info) = ui_data.debug_info {
            info.fps = if self.settings.show_fps {
                self.fps_tracker.fps()
            } else {
                0.0
            };
            info.speed_mode_label = self.speed_mode_label();
            info.frames_in_flight = self.frames_in_flight;
            info.tilt_is_mbc7 = self.cached_is_mbc7;
            info.tilt_stick_controls_tilt =
                self.left_stick_controls_tilt(self.cached_is_mbc7);
            info.tilt_left_stick = self.left_stick;
            info.tilt_keyboard = self.host_input.tilt_vector();
            info.tilt_mouse = self.mouse_tilt_vector();
            info.tilt_target = self.smoothed_tilt;
            info.tilt_smoothed = self.smoothed_tilt;
        }

        if let Some(ref viewer_data) = ui_data.viewer_data {
            if self.debug_windows.show_tile_viewer {
                self.debug_windows.update_tile_viewer_dirty_inputs(
                    &viewer_data.vram,
                    &viewer_data.bg_palette_ram,
                    &viewer_data.obj_palette_ram,
                    viewer_data.ppu.bgp,
                    viewer_data.cgb_mode,
                );
            }
            if self.debug_windows.show_tilemap_viewer {
                self.debug_windows.update_tilemap_dirty_inputs(
                    &viewer_data.vram,
                    &viewer_data.bg_palette_ram,
                    viewer_data.ppu,
                    viewer_data.cgb_mode,
                );
            }
        }

        self.cached_ui_data = Some(ui_data);
    }

    fn tick(&mut self) {
        self.sync_speed_setting();
        self.poll_gamepad();

        let tilt_data = self.update_host_tilt_and_stick_mode();

        self.drain_emu_responses();

        if self.rewind_held && self.settings.rewind_enabled {
            if let Some(thread) = &self.emu_thread {
                thread.send(crate::emu_thread::EmuCommand::Rewind);
            }
            if let Some(resp) = self.emu_thread.as_ref().and_then(|t| t.recv_resp()) {
                match resp {
                    crate::emu_thread::EmuResponse::LoadStateOk { framebuffer, .. } => {
                        self.latest_frame = Some(framebuffer);
                    }
                    _ => {}
                }
            }
        } else if self.frames_in_flight < MAX_IN_FLIGHT {
            let now = Instant::now();
            let frames_to_step = self.compute_frames_to_step(now);

            let has_pending = self.debug_step_requested
                || self.debug_continue_requested
                || self.pending_debug_actions.has_pending();

            if frames_to_step > 0 || has_pending {
                if let Some(thread) = &self.emu_thread {
                    let want_viewer_update = match self.speed_mode() {
                        SpeedMode::Normal => true,
                        SpeedMode::FastForward | SpeedMode::Uncapped => {
                            let now = Instant::now();
                            if now.duration_since(self.last_viewer_update)
                                >= VIEWER_UPDATE_INTERVAL
                            {
                                self.last_viewer_update = now;
                                true
                            } else {
                                false
                            }
                        }
                    };

                    let (buttons_pressed, dpad_pressed) =
                        if let Some(player) = &mut self.replay_player {
                            if let Some((buttons, dpad)) = player.next_frame() {
                                (buttons, dpad)
                            } else {
                                // Replay finished
                                self.toast_manager.info("Replay finished");
                                self.replay_player = None;
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

                    if let Some(recorder) = &mut self.replay_recorder {
                        recorder.record_frame(buttons_pressed, dpad_pressed);
                    }

                    let input = crate::emu_thread::FrameInput {
                        frames: frames_to_step,
                        host_tilt: tilt_data.smoothed,
                        buttons_pressed,
                        dpad_pressed,
                        debug_step: std::mem::take(&mut self.debug_step_requested),
                        debug_continue: std::mem::take(&mut self.debug_continue_requested),
                        apu_capture_enabled: self.debug_windows.show_apu_viewer
                            && want_viewer_update,
                        skip_audio: match self.speed_mode() {
                            SpeedMode::Uncapped => true,
                            SpeedMode::FastForward => self.settings.mute_audio_during_fast_forward,
                            SpeedMode::Normal => false,
                        },
                        debug_actions: std::mem::replace(
                            &mut self.pending_debug_actions,
                            DebugUiActions::none(),
                        ),
                        snapshot: crate::emu_thread::SnapshotRequest {
                            want_debug_info: self.debug_windows.show_cpu_debug
                                || self.settings.show_fps,
                            any_viewer_open: self.debug_windows.any_viewer_open()
                                && want_viewer_update,
                            any_vram_viewer_open: self.debug_windows.any_vram_viewer_open()
                                && want_viewer_update,
                            show_oam_viewer: self.debug_windows.show_oam_viewer
                                && want_viewer_update,
                            show_apu_viewer: self.debug_windows.show_apu_viewer
                                && want_viewer_update,
                            show_disassembler: self.debug_windows.show_disassembler
                                && want_viewer_update,
                            show_rom_info: self.debug_windows.show_rom_info
                                && want_viewer_update,
                            show_memory_viewer: self.debug_windows.show_memory_viewer
                                && want_viewer_update,
                            memory_view_start: self.debug_windows.memory_view_start,
                            last_disasm_pc: self.debug_windows.last_disasm_pc,
                        },
                        reusable_framebuffer: self.recycled_framebuffer.take(),
                        reusable_audio_buffer: self.recycled_audio_buffer.take(),
                        reusable_vram_buffer: self.recycled_vram_buffer.take(),
                        reusable_oam_buffer: self.recycled_oam_buffer.take(),
                        reusable_memory_page: self.recycled_memory_page.take(),
                        active_cheats: crate::cheats::collect_active_cheats(
                            &self.debug_windows.cheats,
                        ),
                        rewind_enabled: self.settings.rewind_enabled && !self.rewind_held,
                    };
                    thread.send(crate::emu_thread::EmuCommand::StepFrames(input));
                    self.frames_in_flight += 1;
                }

                if frames_to_step > 0 {
                    self.fps_tracker.tick();
                }
            }
        }

        let should_render = match self.speed_mode() {
            SpeedMode::Normal => true,
            SpeedMode::FastForward | SpeedMode::Uncapped => {
                let now = Instant::now();
                if now.duration_since(self.last_render_time) >= UI_RENDER_INTERVAL {
                    self.last_render_time = now;
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
            self.recycled_framebuffer = Some(frame);
        }

        crate::debug::sync_show_flags(&mut self.debug_windows, &self.debug_dock);
        self.debug_windows.enable_memory_editing = self.settings.enable_memory_editing;


        let ui_frame_data = self.cached_ui_data.take();
        let rendered = self.render_frame(ui_frame_data.as_ref());
        self.cached_ui_data = ui_frame_data;
        if !rendered {
            return;
        }

        self.tile_viewer_was_open = self.debug_windows.show_tile_viewer;
        self.tilemap_viewer_was_open = self.debug_windows.show_tilemap_viewer;
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.handle_resumed(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        self.handle_window_event(event_loop, window_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule_next_frame(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.perform_shutdown();
    }
}
