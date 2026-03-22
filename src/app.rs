use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

use crate::{
    audio::AudioOutput,
    debug::{DebugWindowState, FpsTracker},
    emu_thread::EmuThread,
    emulator::Emulator,
    graphics::Graphics,
    input::GamepadHandler,
    settings::{LeftStickMode, Settings},
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
    let mut app = App {
        emulator: emulator.map(|emu| Arc::new(Mutex::new(emu))),
        emu_thread: None,
        audio: None,
        gamepad: GamepadHandler::new(),
        gfx: None,
        window_id: None,
        fps_tracker: FpsTracker::new(),
        debug_windows: DebugWindowState::new(),
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
        tile_viewer_was_open: false,
        tilemap_viewer_was_open: false,
        shutdown_performed: false,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    emulator: Option<Arc<Mutex<Emulator>>>,
    emu_thread: Option<EmuThread>,
    audio: Option<AudioOutput>,
    gamepad: Option<GamepadHandler>,
    gfx: Option<Graphics>,
    window_id: Option<WindowId>,
    fps_tracker: FpsTracker,
    debug_windows: DebugWindowState,
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
        let is_mbc7 = self.current_rom_is_mbc7();
        let keyboard = self.host_input.tilt_vector();
        let mouse = self.mouse_tilt_vector();
        let left_stick = self.left_stick;

        self.sync_host_input_with_stick_mode(is_mbc7);
        let target = self.compute_target_tilt(is_mbc7, keyboard, mouse, left_stick);
        let smoothed = self.update_smoothed_tilt(target, is_mbc7);
        self.update_emulator_tilt(smoothed);

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

    fn tick(&mut self) {
        self.update_debug_cache_edges();
        self.sync_speed_setting();
        self.poll_gamepad();

        let tilt_data = self.update_host_tilt_and_stick_mode();
        let now = Instant::now();
        let frames_to_step = self.compute_frames_to_step(now);

        self.apply_debug_run_control();

        if frames_to_step > 0 {
            if let Some(thread) = &self.emu_thread {
                thread.send_step_frames(frames_to_step, tilt_data.smoothed);
            }
        }

        self.drain_emu_responses(matches!(self.speed_mode(), SpeedMode::FastForward));

        if frames_to_step > 0 {
            self.fps_tracker.tick();
        }

        if let Some(frame) = self.latest_frame.take() {
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.upload_framebuffer(&frame);
            }
        }

        let ui_frame_data = self.build_ui_frame_data(tilt_data);
        if !self.render_frame(ui_frame_data) {
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
