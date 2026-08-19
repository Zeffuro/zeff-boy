use anyhow::Result;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};
use zeff_emu_common::address::Address;

use crate::platform::Instant;

use crate::{
    audio::AudioOutput,
    debug::{
        DebugTab, DebugUiActions, DebugWindowState, FpsTracker, ToastManager, restore_dock_layout,
    },
    emu_backend::{ActiveSystem, EmuBackend},
    emu_thread::{EmuResponse, EmuThread},
    graphics::Graphics,
    input::GamepadHandler,
    settings::{DebugPresentation, LeftStickMode, Settings},
    ui,
};

pub(super) use crate::camera::{CameraCapture, CameraHostSettings};

mod bindings;
mod camera_host;
mod display;
mod frame_result;
mod input;
mod keyboard;
mod lifecycle;
mod link;
mod media;
#[cfg(not(target_arch = "wasm32"))]
mod remote;
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

pub(crate) use state_io::detect_and_extract_rom;

pub(crate) fn run(backend: Option<EmuBackend>, settings: Settings) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let uncapped_speed = settings.emulation.uncapped_speed;
    let vsync_mode = settings.video.vsync_mode;
    let initial_audio_output_sample_rate = settings.audio.output_sample_rate;
    let initial_debug_presentation = effective_debug_presentation(settings.ui.debug_presentation);
    let initial_debug_dock = restore_dock_layout(
        initial_debug_presentation,
        settings.ui.dock_layout(initial_debug_presentation),
        &settings.ui.open_debug_tabs,
    );
    #[cfg(not(target_arch = "wasm32"))]
    let update_checker = crate::update::UpdateChecker::new(
        settings.ui.check_for_updates,
        settings.ui.skipped_update_version.clone(),
    );

    // Cache metadata before handing emulator to emu thread
    let cached_is_mbc7 = backend.as_ref().is_some_and(|b| b.is_mbc7());
    let cached_is_pocket_camera = backend.as_ref().is_some_and(|b| b.is_pocket_camera());
    let cached_rom_path = backend.as_ref().map(|b| b.rom_path().to_path_buf());
    let cached_source_path = backend.as_ref().map(|b| b.source_path().to_path_buf());
    let initial_ws_display_rotated = backend.as_ref().and_then(|b| b.ws()).is_some_and(|ws| {
        ws.preferred_orientation() == zeff_ws_core::hardware::cartridge::RomOrientation::Vertical
    });
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
        pending_nes_palette_load: std::rc::Rc::new(std::cell::RefCell::new(None)),
        #[cfg(target_arch = "wasm32")]
        wasm_tab_visible: std::rc::Rc::new(std::cell::Cell::new(true)),
        #[cfg(target_arch = "wasm32")]
        wasm_tab_was_visible: true,
        window_id: None,
        fps_tracker: FpsTracker::new(),
        debug_windows: DebugWindowState::new(),
        debug_dock: initial_debug_dock,
        active_debug_presentation: initial_debug_presentation,
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
        mouse_left_pressed: false,
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
        last_core_frame: None,
        last_displayed_frame: None,
        recycled: RecycledBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
        frames_in_flight: 0,
        cached_ui_data: None,
        rom_info: CachedRomInfo {
            is_mbc7: cached_is_mbc7,
            is_pocket_camera: cached_is_pocket_camera,
            rom_path: cached_rom_path,
            source_path: cached_source_path,
            rom_hash: None,
            replay_metadata: None,
        },
        symbols: crate::symbols::SymbolSession::default(),
        #[cfg(not(target_arch = "wasm32"))]
        pending_symbol_load: None,
        #[cfg(not(target_arch = "wasm32"))]
        next_symbol_load_id: 0,
        nes_palette_cache: NesPaletteFileCache::default(),
        pending_archive_selection: None,
        pending_debug_actions: DebugUiActions::none(),
        shutdown_performed: false,
        toast_manager: ToastManager::new(),
        #[cfg(not(target_arch = "wasm32"))]
        update_checker,
        recording: RecordingState {
            audio_recorder: None,
            replay_recorder: None,
            #[cfg(not(target_arch = "wasm32"))]
            pending_replay_start: None,
            #[cfg(not(target_arch = "wasm32"))]
            next_replay_capture_id: 0,
            #[cfg(not(target_arch = "wasm32"))]
            replay_finalization: None,
            replay_player: None,
            pending_replay_batches: std::collections::VecDeque::new(),
            queued_replay_playback_frames: 0,
            replay_recording_origin: ReplayCaptureOrigin::default(),
            replay_media_events_pending: 0,
            last_replay_checkpoint_frame: 0,
            pending_replay_checkpoint_hashes: std::collections::BTreeMap::new(),
        },
        rewind: RewindState {
            held: false,
            fill: 0.0,
            throttle: 0,
            pops: 0,
            pending: false,
            backstep_pending: false,
        },
        remote_debug_frames_remaining: 0,
        remote_memory_view_start: None,
        remote_memory_frames_remaining: 0,
        remote_graphics_frames_remaining: 0,
        remote_zapper: None,
        #[cfg(not(target_arch = "wasm32"))]
        live_control: crate::live_control::LiveControl::from_env(),
        #[cfg(not(target_arch = "wasm32"))]
        live_button_releases: Vec::new(),
        #[cfg(not(target_arch = "wasm32"))]
        tcp_link_active: false,
        window_focused: true,
        game_window_focused: true,
        #[cfg(not(target_arch = "wasm32"))]
        debugger_window_focused: false,
        #[cfg(not(target_arch = "wasm32"))]
        settings_window_focused: false,
        focus_state_dirty: false,
        #[cfg(not(target_arch = "wasm32"))]
        last_debugger_render: Instant::now(),
        #[cfg(not(target_arch = "wasm32"))]
        last_settings_render: Instant::now(),
        egui_wants_keyboard: false,
        game_view_focused: true,
        active_system,
        ws_display_rotated: initial_ws_display_rotated,
        cached_slot_info: state_io::SlotInfo {
            labels: std::array::from_fn(|i| format!("Slot {i}  (empty)")),
            occupied: [false; 10],
        },
        undo_load_state: None,
        paused_by_unfocus: false,
        suppress_unfocus_pause_until_focus: false,
    };

    app.debug_windows.memory.configure_for_system(active_system);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((system, rom_path, source_path, rom_hash)) = app.initial_backend.as_ref().map(|b| {
        (
            b.system(),
            b.rom_path().to_path_buf(),
            b.source_path().to_path_buf(),
            b.rom_hash(),
        )
    }) {
        app.start_symbol_load_for_paths(system, rom_path, source_path, rom_hash);
    }

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
    pending_nes_palette_load: crate::platform::FileDataSlot,
    #[cfg(target_arch = "wasm32")]
    wasm_tab_visible: std::rc::Rc<std::cell::Cell<bool>>,
    #[cfg(target_arch = "wasm32")]
    wasm_tab_was_visible: bool,
    window_id: Option<WindowId>,
    fps_tracker: FpsTracker,
    debug_windows: DebugWindowState,
    debug_dock: egui_dock::DockState<DebugTab>,
    active_debug_presentation: DebugPresentation,
    exit_requested: bool,
    settings: Settings,
    timing: TimingState,
    last_audio_output_sample_rate: u32,
    speed: SpeedState,
    modifiers: ModifierKeys,
    host_input: HostInputState,
    cursor_pos: Option<(f32, f32)>,
    mouse_left_pressed: bool,
    window_size: (f32, f32),
    tilt: TiltState,
    camera: CameraState,
    last_state_dir: Option<std::path::PathBuf>,
    show_settings_window: bool,
    debug_requests: DebugRequests,
    active_save_slot: u8,
    latest_frame: Option<Arc<Vec<u8>>>,
    last_core_frame: Option<Arc<Vec<u8>>>,
    last_displayed_frame: Option<Arc<Vec<u8>>>,
    recycled: RecycledBuffers,
    frames_in_flight: usize,
    cached_ui_data: Option<ui::UiFrameData>,
    rom_info: CachedRomInfo,
    symbols: crate::symbols::SymbolSession,
    #[cfg(not(target_arch = "wasm32"))]
    pending_symbol_load: Option<PendingSymbolLoad>,
    #[cfg(not(target_arch = "wasm32"))]
    next_symbol_load_id: u64,
    nes_palette_cache: NesPaletteFileCache,
    pending_archive_selection: Option<PendingArchiveSelection>,
    pending_debug_actions: DebugUiActions,
    shutdown_performed: bool,
    toast_manager: ToastManager,
    #[cfg(not(target_arch = "wasm32"))]
    update_checker: crate::update::UpdateChecker,
    recording: RecordingState,
    rewind: RewindState,
    remote_debug_frames_remaining: usize,
    remote_memory_view_start: Option<Address>,
    remote_memory_frames_remaining: usize,
    remote_graphics_frames_remaining: usize,
    remote_zapper: Option<crate::emu_thread::ZapperInput>,
    #[cfg(not(target_arch = "wasm32"))]
    live_control: crate::live_control::LiveControl,
    #[cfg(not(target_arch = "wasm32"))]
    live_button_releases: Vec<crate::live_control::PendingButtonRelease>,
    #[cfg(not(target_arch = "wasm32"))]
    tcp_link_active: bool,
    window_focused: bool,
    game_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    debugger_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    settings_window_focused: bool,
    focus_state_dirty: bool,
    #[cfg(not(target_arch = "wasm32"))]
    last_debugger_render: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_settings_render: Instant,
    egui_wants_keyboard: bool,
    game_view_focused: bool,
    active_system: ActiveSystem,
    ws_display_rotated: bool,
    cached_slot_info: state_io::SlotInfo,
    undo_load_state: Option<Vec<u8>>,
    paused_by_unfocus: bool,
    suppress_unfocus_pause_until_focus: bool,
}

fn effective_debug_presentation(presentation: DebugPresentation) -> DebugPresentation {
    #[cfg(not(target_arch = "wasm32"))]
    if crate::live_control::automation_mode_enabled() {
        return DebugPresentation::Floating;
    }

    #[cfg(target_arch = "wasm32")]
    if presentation == DebugPresentation::GameAndDebugger {
        return DebugPresentation::Floating;
    }

    presentation
}

fn activate_debug_presentation_state(
    active: &mut DebugPresentation,
    dock: &mut egui_dock::DockState<DebugTab>,
    settings: &mut Settings,
    desired: DebugPresentation,
) -> bool {
    if desired == *active {
        return false;
    }

    if let Some(layout) = crate::debug::serialize_dock_layout(dock) {
        settings.ui.set_dock_layout(*active, layout);
    }
    *active = desired;
    *dock = crate::debug::restore_dock_layout(desired, settings.ui.dock_layout(desired), &[]);
    true
}

impl App {
    fn activate_debug_presentation(&mut self, desired: DebugPresentation) -> bool {
        activate_debug_presentation_state(
            &mut self.active_debug_presentation,
            &mut self.debug_dock,
            &mut self.settings,
            desired,
        )
    }

    fn debug_workspace_visible(&self) -> bool {
        if self.active_debug_presentation != DebugPresentation::GameAndDebugger {
            return true;
        }

        #[cfg(not(target_arch = "wasm32"))]
        return self.settings.ui.debugger_window_open
            && self
                .gfx
                .as_ref()
                .and_then(Graphics::debugger_window)
                .is_some_and(|window| window.is_minimized() != Some(true));

        #[cfg(target_arch = "wasm32")]
        false
    }

    fn speed_mode(&self) -> SpeedMode {
        if self.timing.uncapped_speed {
            SpeedMode::Uncapped
        } else if self.speed.fast_forward_held {
            SpeedMode::FastForward
        } else if self.settings.emulation.slow_motion_enabled {
            SpeedMode::SlowMotion
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
            SpeedMode::SlowMotion => "Slow",
            SpeedMode::Uncapped => "Uncapped (Benchmark)",
            SpeedMode::FastForward => "Fast",
        }
    }

    fn effective_frame_duration(&self) -> std::time::Duration {
        let base = std::time::Duration::from_nanos(self.active_system.frame_duration_ns());
        match self.speed_mode() {
            SpeedMode::FastForward => {
                let multi = self.settings.emulation.fast_forward_multiplier.max(1) as u32;
                base / multi
            }
            SpeedMode::SlowMotion => {
                let divisor = self.settings.emulation.slow_motion_divisor.clamp(2, 16) as u32;
                base.saturating_mul(divisor)
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
        loop {
            while let Some(result) = self.emu_thread.as_ref()?.try_recv_frame() {
                self.process_frame_result(result);
            }
            let response = self.emu_thread.as_ref()?.recv()?;
            if self.handle_link_response(&response) {
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            let response = match self.consume_replay_start_response(response) {
                Some(response) => response,
                None => continue,
            };
            #[cfg(not(target_arch = "wasm32"))]
            let response = match self.consume_replay_finalization_response(response) {
                Some(response) => response,
                None => continue,
            };
            return Some(response);
        }
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.drain_live_control();
            self.sync_debug_presentation(event_loop);
            self.sync_settings_window(event_loop);
        }
        self.apply_focus_state();
        self.schedule_next_frame(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.perform_shutdown();
    }
}

#[cfg(test)]
mod presentation_tests {
    use super::*;

    fn make_restorable(mut value: serde_json::Value) -> serde_json::Value {
        fn visit(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(fields) => {
                    for (key, value) in fields {
                        if matches!(key.as_str(), "x" | "y") && value.is_null() {
                            *value = serde_json::json!(0.0);
                        } else {
                            visit(value);
                        }
                    }
                }
                serde_json::Value::Array(values) => values.iter_mut().for_each(visit),
                _ => {}
            }
        }

        visit(&mut value);
        value
    }

    #[test]
    fn presentation_switch_persists_and_restores_each_dock() {
        let mut settings = Settings::default();
        let mut active = DebugPresentation::Floating;
        let floating_layout = make_restorable(
            crate::debug::serialize_dock_layout(&crate::debug::create_default_dock_state())
                .unwrap(),
        );
        let mut dock = serde_json::from_value(floating_layout.clone()).unwrap();
        let floating_layout = crate::debug::serialize_dock_layout(&dock).unwrap();
        let mut ide = crate::debug::create_ide_dock_state();
        let memory = ide.find_tab(&DebugTab::MemoryViewer).unwrap();
        ide.remove_tab(memory);
        let ide_layout = make_restorable(crate::debug::serialize_dock_layout(&ide).unwrap());
        settings
            .ui
            .set_dock_layout(DebugPresentation::Ide, ide_layout.clone());

        assert!(activate_debug_presentation_state(
            &mut active,
            &mut dock,
            &mut settings,
            DebugPresentation::Ide,
        ));
        assert_eq!(active, DebugPresentation::Ide);
        assert_eq!(crate::debug::serialize_dock_layout(&dock), Some(ide_layout));
        assert_eq!(
            settings.ui.dock_layout(DebugPresentation::Floating),
            Some(&floating_layout)
        );
        assert!(activate_debug_presentation_state(
            &mut active,
            &mut dock,
            &mut settings,
            DebugPresentation::Floating,
        ));
        assert_eq!(active, DebugPresentation::Floating);
        assert_eq!(
            crate::debug::serialize_dock_layout(&dock),
            Some(floating_layout)
        );
        assert!(!activate_debug_presentation_state(
            &mut active,
            &mut dock,
            &mut settings,
            DebugPresentation::Floating,
        ));
    }
}
