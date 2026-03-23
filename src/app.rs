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
    debug::{DebugUiActions, DebugWindowState, FpsTracker, create_default_dock_state},
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
        debug_dock: create_default_dock_state(),
        exit_requested: false,
        settings,
        last_frame_time: Instant::now(),
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
        frame_in_flight: false,
        cached_ui_data: None,
        cached_is_mbc7,
        cached_rom_path,
        pending_debug_actions: DebugUiActions::none(),
        tile_viewer_was_open: false,
        tilemap_viewer_was_open: false,
        shutdown_performed: false,
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
    
    frame_in_flight: bool,
    
    cached_ui_data: Option<ui::UiFrameData>,
    
    cached_is_mbc7: bool,
    
    cached_rom_path: Option<PathBuf>,
    
    pending_debug_actions: DebugUiActions,
    tile_viewer_was_open: bool,
    tilemap_viewer_was_open: bool,
    shutdown_performed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeedMode {
    Normal,
    Uncapped,
    FastForward,
}

const GB_FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);

impl App {

    fn speed_mode(&self) -> SpeedMode {
        if self.fast_forward_held {
            SpeedMode::FastForward
        } else if self.uncapped_speed {
            SpeedMode::Uncapped
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
        loop {
            let resp = match &self.emu_thread {
                Some(thread) => thread.recv(),
                None => return None,
            };
            match resp {
                Some(crate::emu_thread::EmuResponse::FrameReady(result)) => {
                    self.process_frame_result(result);
                }
                other => return other,
            }
        }
    }

    fn process_frame_result(&mut self, result: crate::emu_thread::FrameResult) {
        self.frame_in_flight = false;
        self.latest_frame = Some(result.frame);
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

        let mut ui_data = result.ui_data;
        if let Some(ref mut info) = ui_data.debug_info {
            info.fps = if self.settings.show_fps {
                self.fps_tracker.fps()
            } else {
                0.0
            };
            info.speed_mode_label = self.speed_mode_label();
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
        self.update_debug_cache_edges();
        self.sync_speed_setting();
        self.poll_gamepad();

        let tilt_data = self.update_host_tilt_and_stick_mode();
        
        self.drain_emu_responses();
        
        if !self.frame_in_flight {
            let now = Instant::now();
            let frames_to_step = self.compute_frames_to_step(now);

            if let Some(thread) = &self.emu_thread {
                let input = crate::emu_thread::FrameInput {
                    frames: frames_to_step,
                    host_tilt: tilt_data.smoothed,
                    buttons_pressed: self.host_input.buttons_pressed(),
                    dpad_pressed: self.host_input.dpad_pressed(),
                    debug_step: std::mem::take(&mut self.debug_step_requested),
                    debug_continue: std::mem::take(&mut self.debug_continue_requested),
                    apu_capture_enabled: self.debug_windows.show_apu_viewer,
                    debug_actions: std::mem::replace(
                        &mut self.pending_debug_actions,
                        DebugUiActions::none(),
                    ),
                    snapshot: crate::emu_thread::SnapshotRequest {
                        any_viewer_open: self.debug_windows.any_viewer_open(),
                        any_vram_viewer_open: self.debug_windows.any_vram_viewer_open(),
                        show_apu_viewer: self.debug_windows.show_apu_viewer,
                        show_disassembler: self.debug_windows.show_disassembler,
                        show_rom_info: self.debug_windows.show_rom_info,
                        show_memory_viewer: self.debug_windows.show_memory_viewer,
                        memory_view_start: self.debug_windows.memory_view_start,
                    },
                };
                thread.send(crate::emu_thread::EmuCommand::StepFrames(input));
                self.frame_in_flight = true;
            }

            if frames_to_step > 0 {
                self.fps_tracker.tick();
            }
        }

        if let Some(frame) = self.latest_frame.take() {
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.upload_framebuffer(&frame);
            }
        }

        crate::debug::sync_show_flags(&mut self.debug_windows, &self.debug_dock);


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
