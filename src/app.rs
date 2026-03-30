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
    debug::{
        DebugTab, DebugUiActions, DebugWindowState, FpsTracker, ToastManager,
        create_default_dock_state, create_dock_from_saved_tabs,
    },
    emu_backend::{ActiveSystem, EmuBackend},
    emu_thread::{EmuResponse, EmuThread},
    graphics::Graphics,
    input::GamepadHandler,
    settings::{LeftStickMode, Settings},
    ui,
};

pub(super) use crate::camera::{CameraCapture, CameraHostSettings};

mod bindings;
mod camera_host;
mod input;
mod keyboard;
mod lifecycle;
mod shutdown;
mod state_io;
mod tick;
mod tilt;
mod window_events;

use input::HostInputState;
use tilt::{AutoTiltSource, TiltConfig};

pub(crate) use state_io::extract_rom_from_zip;

pub(crate) fn run(backend: Option<EmuBackend>, settings: Settings) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let uncapped_speed = settings.emulation.uncapped_speed;
    let vsync_mode = settings.video.vsync_mode;

    // Cache metadata before handing emulator to emu thread
    let cached_is_mbc7 = backend.as_ref().is_some_and(|b| b.is_mbc7());
    let cached_is_pocket_camera = backend.as_ref().is_some_and(|b| b.is_pocket_camera());
    let cached_rom_path = backend.as_ref().map(|b| b.rom_path().to_path_buf());
    let active_system = backend.as_ref().map(|b| b.system()).unwrap_or(ActiveSystem::GameBoy);

    let mut app = App {
        emu_thread: None,
        initial_backend: backend,
        audio: None,
        gamepad: GamepadHandler::new()
            .map_err(|e| log::error!("Gamepad init failed: {e}"))
            .ok(),
        gfx: None,
        window_id: None,
        fps_tracker: FpsTracker::new(),
        debug_windows: DebugWindowState::new(),
        debug_dock: if settings.ui.open_debug_tabs.is_empty() {
            create_default_dock_state()
        } else {
            create_dock_from_saved_tabs(&settings.ui.open_debug_tabs)
        },
        exit_requested: false,
        settings,
        timing: TimingState {
            last_frame_time: Instant::now(),
            last_render_time: Instant::now(),
            last_viewer_update: Instant::now(),
            uncapped_speed,
            last_vsync_mode: vsync_mode,
        },
        fast_forward_held: false,
        turbo_held: false,
        turbo_counter: 0,
        modifiers: ModifierKeys::default(),
        host_input: HostInputState::new(),
        cursor_pos: None,
        window_size: (160.0, 144.0),
        smoothed_tilt: (0.0, 0.0),
        left_stick: (0.0, 0.0),
        auto_tilt_source: AutoTiltSource::Keyboard,
        camera_capture: None,
        camera_capture_index: None,
        last_state_dir: None,
        show_settings_window: false,
        debug_requests: DebugRequests::default(),
        active_save_slot: 0,
        latest_frame: None,
        last_displayed_frame: None,
        recycled: RecycledBuffers {
            framebuffer: None,
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
        },
        frames_in_flight: 0,
        cached_ui_data: None,
        cached_is_mbc7,
        cached_is_pocket_camera,
        cached_rom_path,
        cached_rom_hash: None,
        pending_debug_actions: DebugUiActions::none(),
        tile_viewer_was_open: false,
        tilemap_viewer_was_open: false,
        shutdown_performed: false,
        toast_manager: ToastManager::new(),
        recording: RecordingState {
            audio_recorder: None,
            replay_recorder: None,
            replay_player: None,
        },
        paused: false,
        rewind: RewindState {
            held: false,
            fill: 0.0,
            throttle: 0,
            pops: 0,
            pending: false,
            backstep_pending: false,
        },
        egui_wants_keyboard: false,
        active_system,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct RecycledBuffers {
    framebuffer: Option<Vec<u8>>,
    audio: Option<Vec<f32>>,
    vram: Option<Vec<u8>>,
    oam: Option<Vec<u8>>,
    memory_page: Option<Vec<(u16, u8)>>,
}

impl RecycledBuffers {
    fn clear(&mut self) {
        self.framebuffer = None;
        self.audio = None;
        self.vram = None;
        self.oam = None;
        self.memory_page = None;
    }
}

struct RewindState {
    held: bool,
    fill: f32,
    throttle: usize,
    pops: usize,
    pending: bool,
    backstep_pending: bool,
}

struct RecordingState {
    audio_recorder: Option<crate::audio_recorder::AudioRecorder>,
    replay_recorder: Option<zeff_gb_core::replay::ReplayRecorder>,
    replay_player: Option<zeff_gb_core::replay::ReplayPlayer>,
}

struct TimingState {
    last_frame_time: Instant,
    last_render_time: Instant,
    last_viewer_update: Instant,
    uncapped_speed: bool,
    last_vsync_mode: crate::settings::VsyncMode,
}

#[derive(Default)]
struct ModifierKeys {
    shift: bool,
    ctrl: bool,
    alt: bool,
}

#[derive(Default)]
struct DebugRequests {
    step: bool,
    continue_: bool,
    backstep: bool,
    frame_advance: bool,
}

impl DebugRequests {

    fn has_pending(&self) -> bool {
        self.step || self.continue_ || self.backstep || self.frame_advance
    }
}

struct App {
    initial_backend: Option<EmuBackend>,
    emu_thread: Option<EmuThread>,
    audio: Option<AudioOutput>,
    gamepad: Option<GamepadHandler>,
    gfx: Option<Graphics>,
    window_id: Option<WindowId>,
    fps_tracker: FpsTracker,
    debug_windows: DebugWindowState,
    debug_dock: egui_dock::DockState<DebugTab>,
    exit_requested: bool,
    settings: Settings,
    timing: TimingState,
    fast_forward_held: bool,
    turbo_held: bool,
    turbo_counter: u8,
    modifiers: ModifierKeys,
    host_input: HostInputState,
    cursor_pos: Option<(f32, f32)>,
    window_size: (f32, f32),
    smoothed_tilt: (f32, f32),
    left_stick: (f32, f32),
    auto_tilt_source: AutoTiltSource,
    camera_capture: Option<CameraCapture>,
    camera_capture_index: Option<u32>,
    last_state_dir: Option<PathBuf>,
    show_settings_window: bool,
    debug_requests: DebugRequests,
    active_save_slot: u8,
    latest_frame: Option<Vec<u8>>,
    last_displayed_frame: Option<Vec<u8>>,
    recycled: RecycledBuffers,
    frames_in_flight: usize,
    cached_ui_data: Option<ui::UiFrameData>,
    cached_is_mbc7: bool,
    cached_is_pocket_camera: bool,
    cached_rom_path: Option<PathBuf>,
    cached_rom_hash: Option<[u8; 32]>,
    pending_debug_actions: DebugUiActions,
    tile_viewer_was_open: bool,
    tilemap_viewer_was_open: bool,
    shutdown_performed: bool,
    toast_manager: ToastManager,
    recording: RecordingState,
    paused: bool,
    rewind: RewindState,
    egui_wants_keyboard: bool,
    active_system: ActiveSystem,
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

/// NES runs at ~60.0988 fps → 16_639_267 ns per frame
const NES_FRAME_DURATION: Duration = Duration::from_nanos(16_639_267);

impl App {
    fn speed_mode(&self) -> SpeedMode {
        if self.timing.uncapped_speed {
            SpeedMode::Uncapped
        } else if self.fast_forward_held {
            SpeedMode::FastForward
        } else {
            SpeedMode::Normal
        }
    }

    fn speed_mode_label(&self) -> &'static str {
        if self.paused {
            return "Paused";
        }
        match self.speed_mode() {
            SpeedMode::Normal => "Normal",
            SpeedMode::Uncapped => "Uncapped (Benchmark)",
            SpeedMode::FastForward => "Fast",
        }
    }

    fn effective_frame_duration(&self) -> Duration {
        let base = match self.active_system {
            ActiveSystem::GameBoy => GB_FRAME_DURATION,
            ActiveSystem::Nes => NES_FRAME_DURATION,
        };
        match self.speed_mode() {
            SpeedMode::FastForward => {
                let multi = self.settings.emulation.fast_forward_multiplier.max(1) as u32;
                base / multi
            }
            _ => base,
        }
    }

    fn left_stick_controls_tilt(&self, is_mbc7: bool) -> bool {
        match self.settings.tilt.left_stick_mode {
            LeftStickMode::Tilt => true,
            LeftStickMode::Dpad => false,
            LeftStickMode::Auto => is_mbc7,
        }
    }

    fn left_stick_controls_dpad(&self, is_mbc7: bool) -> bool {
        !self.left_stick_controls_tilt(is_mbc7)
    }

    fn sync_host_input_with_stick_mode(&mut self, is_mbc7: bool) {
        if self.left_stick_controls_dpad(is_mbc7) {
            self.host_input
                .set_gamepad_stick_dpad(self.left_stick, self.settings.tilt.deadzone);
        } else {
            self.host_input.clear_gamepad_stick_dpad();
        }
    }

    fn mouse_tilt_vector(&self) -> (f32, f32) {
        tilt::mouse_tilt_vector(self.cursor_pos, self.window_size)
    }

    fn tilt_config(&self) -> TiltConfig {
        TiltConfig {
            sensitivity: self.settings.tilt.sensitivity,
            invert_x: self.settings.tilt.invert_x,
            invert_y: self.settings.tilt.invert_y,
            deadzone: self.settings.tilt.deadzone,
            stick_bypass_lerp: self.settings.tilt.stick_bypass_lerp,
            lerp: self.settings.tilt.lerp,
        }
    }

    fn compute_target_tilt(
        &mut self,
        is_mbc7: bool,
        keyboard: (f32, f32),
        mouse: (f32, f32),
        left_stick: (f32, f32),
    ) -> (f32, f32) {
        let stick_controls_tilt = self.left_stick_controls_tilt(is_mbc7);
        let cfg = self.tilt_config();
        tilt::compute_target_tilt(
            is_mbc7,
            self.settings.tilt.input_mode,
            &mut self.auto_tilt_source,
            &tilt::TiltInputSources { keyboard, mouse, left_stick },
            stick_controls_tilt,
            &cfg,
        )
    }

    fn update_smoothed_tilt(&mut self, target: (f32, f32), is_mbc7: bool) -> (f32, f32) {
        let stick_controls_tilt = self.left_stick_controls_tilt(is_mbc7);
        let cfg = self.tilt_config();
        tilt::update_smoothed_tilt(
            &mut self.smoothed_tilt,
            target,
            is_mbc7,
            self.left_stick,
            stick_controls_tilt,
            &cfg,
        )
    }

    fn update_host_tilt_and_stick_mode(&mut self) -> (f32, f32) {
        let is_mbc7 = self.cached_is_mbc7;
        let keyboard = self.host_input.tilt_vector();
        let mouse = self.mouse_tilt_vector();
        let left_stick = self.left_stick;

        self.sync_host_input_with_stick_mode(is_mbc7);
        let target = self.compute_target_tilt(is_mbc7, keyboard, mouse, left_stick);
        self.update_smoothed_tilt(target, is_mbc7)
    }

    fn recv_cold_response(&mut self) -> Option<EmuResponse> {
        while let Some(result) = self.emu_thread.as_ref()?.try_recv_frame() {
            self.process_frame_result(result);
        }
        self.emu_thread.as_ref()?.recv()
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
