use anyhow::Result;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

use crate::platform::Instant;

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
mod frame_result;
mod input;
mod keyboard;
mod lifecycle;
mod render;
mod shutdown;
mod state_io;
mod tick;
mod tilt;
mod types;
mod window_events;

use input::HostInputState;
use tilt::{AutoTiltSource, TiltConfig};
use types::*;

#[cfg(target_arch = "wasm32")]
type PendingGfx = Option<std::rc::Rc<std::cell::RefCell<Option<anyhow::Result<Graphics>>>>>;

pub(crate) use state_io::extract_rom_from_zip;

pub(crate) fn run(backend: Option<EmuBackend>, settings: Settings) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let uncapped_speed = settings.emulation.uncapped_speed;
    let vsync_mode = settings.video.vsync_mode;
    let initial_audio_output_sample_rate = settings.audio.output_sample_rate;

    // Cache metadata before handing emulator to emu thread
    let cached_is_mbc7 = backend.as_ref().is_some_and(|b| b.is_mbc7());
    let cached_is_pocket_camera = backend.as_ref().is_some_and(|b| b.is_pocket_camera());
    let cached_rom_path = backend.as_ref().map(|b| b.rom_path().to_path_buf());
    let active_system = backend
        .as_ref()
        .map(|b| b.system())
        .unwrap_or(ActiveSystem::GameBoy);

    #[allow(unused_mut)]
    let mut app = App {
        emu_thread: None,
        initial_backend: backend,
        audio: None,
        gamepad: GamepadHandler::new()
            .map_err(|e| log::error!("Gamepad init failed: {e}"))
            .ok(),
        gfx: None,
        #[cfg(target_arch = "wasm32")]
        pending_gfx: None,
        #[cfg(target_arch = "wasm32")]
        pending_rom_load: std::rc::Rc::new(std::cell::RefCell::new(None)),
        #[cfg(target_arch = "wasm32")]
        pending_state_load: std::rc::Rc::new(std::cell::RefCell::new(None)),
        #[cfg(target_arch = "wasm32")]
        wasm_tab_visible: std::rc::Rc::new(std::cell::Cell::new(true)),
        #[cfg(target_arch = "wasm32")]
        wasm_tab_was_visible: true,
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
        last_audio_output_sample_rate: initial_audio_output_sample_rate,
        speed: SpeedState {
            paused: false,
            fast_forward_held: false,
            turbo_held: false,
            turbo_counter: 0,
        },
        modifiers: ModifierKeys::default(),
        host_input: HostInputState::new(),
        cursor_pos: None,
        window_size: (160.0, 144.0),
        tilt: TiltState {
            smoothed: (0.0, 0.0),
            left_stick: (0.0, 0.0),
            auto_source: AutoTiltSource::Keyboard,
        },
        camera: CameraState {
            capture: None,
            capture_index: None,
        },
        last_state_dir: None,
        show_settings_window: false,
        debug_requests: DebugRequests::default(),
        active_save_slot: 0,
        latest_frame: None,
        last_displayed_frame: None,
        recycled: RecycledBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
        },
        frames_in_flight: 0,
        cached_ui_data: None,
        rom_info: CachedRomInfo {
            is_mbc7: cached_is_mbc7,
            is_pocket_camera: cached_is_pocket_camera,
            rom_path: cached_rom_path,
            rom_hash: None,
        },
        pending_debug_actions: DebugUiActions::none(),
        shutdown_performed: false,
        toast_manager: ToastManager::new(),
        recording: RecordingState {
            audio_recorder: None,
            replay_recorder: None,
            replay_player: None,
        },
        rewind: RewindState {
            held: false,
            fill: 0.0,
            throttle: 0,
            pops: 0,
            pending: false,
            backstep_pending: false,
        },
        egui_wants_keyboard: false,
        game_view_focused: true,
        active_system,
        cached_slot_info: state_io::SlotInfo {
            labels: std::array::from_fn(|i| format!("Slot {i}  (empty)")),
            occupied: [false; 10],
        },
        paused_by_unfocus: false,
    };

    #[cfg(not(target_arch = "wasm32"))]
    event_loop.run_app(&mut app)?;

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
    }

    Ok(())
}

struct App {
    initial_backend: Option<EmuBackend>,
    emu_thread: Option<EmuThread>,
    audio: Option<AudioOutput>,
    gamepad: Option<GamepadHandler>,
    gfx: Option<Graphics>,
    #[cfg(target_arch = "wasm32")]
    pending_gfx: PendingGfx,
    #[cfg(target_arch = "wasm32")]
    pending_rom_load: crate::platform::FileDataSlot,
    #[cfg(target_arch = "wasm32")]
    pending_state_load: crate::platform::FileDataSlot,
    #[cfg(target_arch = "wasm32")]
    wasm_tab_visible: std::rc::Rc<std::cell::Cell<bool>>,
    #[cfg(target_arch = "wasm32")]
    wasm_tab_was_visible: bool,
    window_id: Option<WindowId>,
    fps_tracker: FpsTracker,
    debug_windows: DebugWindowState,
    debug_dock: egui_dock::DockState<DebugTab>,
    exit_requested: bool,
    settings: Settings,
    timing: TimingState,
    last_audio_output_sample_rate: u32,
    speed: SpeedState,
    modifiers: ModifierKeys,
    host_input: HostInputState,
    cursor_pos: Option<(f32, f32)>,
    window_size: (f32, f32),
    tilt: TiltState,
    camera: CameraState,
    last_state_dir: Option<std::path::PathBuf>,
    show_settings_window: bool,
    debug_requests: DebugRequests,
    active_save_slot: u8,
    latest_frame: Option<Arc<Vec<u8>>>,
    last_displayed_frame: Option<Arc<Vec<u8>>>,
    recycled: RecycledBuffers,
    frames_in_flight: usize,
    cached_ui_data: Option<ui::UiFrameData>,
    rom_info: CachedRomInfo,
    pending_debug_actions: DebugUiActions,
    shutdown_performed: bool,
    toast_manager: ToastManager,
    recording: RecordingState,
    rewind: RewindState,
    egui_wants_keyboard: bool,
    game_view_focused: bool,
    active_system: ActiveSystem,
    cached_slot_info: state_io::SlotInfo,
    paused_by_unfocus: bool,
}

impl App {
    fn speed_mode(&self) -> SpeedMode {
        if self.timing.uncapped_speed {
            SpeedMode::Uncapped
        } else if self.speed.fast_forward_held {
            SpeedMode::FastForward
        } else {
            SpeedMode::Normal
        }
    }

    fn refresh_slot_info(&mut self) {
        self.cached_slot_info =
            state_io::build_slot_info(self.rom_info.rom_hash, self.active_system);
    }

    fn speed_mode_label(&self) -> &'static str {
        if self.speed.paused {
            return "Paused";
        }
        match self.speed_mode() {
            SpeedMode::Normal => "Normal",
            SpeedMode::Uncapped => "Uncapped (Benchmark)",
            SpeedMode::FastForward => "Fast",
        }
    }

    fn effective_frame_duration(&self) -> std::time::Duration {
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
                .set_gamepad_stick_dpad(self.tilt.left_stick, self.settings.tilt.deadzone);
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
            &mut self.tilt.auto_source,
            &tilt::TiltInputSources {
                keyboard,
                mouse,
                left_stick,
            },
            stick_controls_tilt,
            &cfg,
        )
    }

    fn update_smoothed_tilt(&mut self, target: (f32, f32), is_mbc7: bool) -> (f32, f32) {
        let stick_controls_tilt = self.left_stick_controls_tilt(is_mbc7);
        let cfg = self.tilt_config();
        tilt::update_smoothed_tilt(
            &mut self.tilt.smoothed,
            target,
            is_mbc7,
            self.tilt.left_stick,
            stick_controls_tilt,
            &cfg,
        )
    }

    fn update_host_tilt_and_stick_mode(&mut self) -> (f32, f32) {
        let is_mbc7 = self.rom_info.is_mbc7;
        let keyboard = self.host_input.tilt_vector();
        let mouse = self.mouse_tilt_vector();
        let left_stick = self.tilt.left_stick;

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
        self.wasm_poll_hooks(event_loop);
        self.schedule_next_frame(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.perform_shutdown();
    }
}
