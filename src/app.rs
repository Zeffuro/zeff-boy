use crate::{
    audio::AudioOutput,
    debug::{
        DebugTab, DebugUiActions, DebugWindowState, FpsTracker, ToastManager, restore_dock_layout,
    },
    emu_backend::{ActiveSystem, EmuBackend},
    emu_thread::EmuThread,
    graphics::Graphics,
    input::GamepadHandler,
    platform::Instant,
    settings::{DebugPresentation, LeftStickMode, Settings},
    ui,
};
use anyhow::Result;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};
use zeff_emu_common::address::Address;

pub(super) use crate::camera::{CameraCapture, CameraHostSettings};

mod autofire;
mod bindings;
#[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
pub(crate) mod browser_speculation_test;
mod camera_host;
mod command_gate;
mod construct;
mod display;
mod frame_result;
mod input;
mod input_configuration;
mod keyboard;
mod lifecycle;
mod link;
mod media;
#[cfg(not(target_arch = "wasm32"))]
mod netplay;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use netplay::proof::run_if_requested as run_netplay_proof_if_requested;
mod pause;
#[cfg(not(target_arch = "wasm32"))]
mod remote;
mod render;
mod serial_devices;
mod shutdown;
mod state_io;
#[cfg(not(target_arch = "wasm32"))]
mod tas_control;
mod tas_editor;
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
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use state_io::detect_and_extract_rom_with_zip_witness;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use state_io::is_native_archive_path;

pub(crate) fn run(
    backend: Option<EmuBackend>,
    settings: Settings,
    #[cfg(not(target_arch = "wasm32"))] deferred_initial_rom_load: Option<std::path::PathBuf>,
) -> Result<()> {
    let event_loop = EventLoop::new()?;
    #[cfg(target_arch = "wasm32")]
    let wasm_event_loop_proxy = event_loop.create_proxy();
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let timer_proxy = wasm_event_loop_proxy.clone();
        let timer = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
            let _ = timer_proxy.send_event(());
        }) as Box<dyn FnMut()>);
        if let Some(window) = web_sys::window() {
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                timer.as_ref().unchecked_ref(),
                1_000,
            );
        }
        timer.forget();
    }
    let app = construct::create(
        backend,
        settings,
        #[cfg(not(target_arch = "wasm32"))]
        deferred_initial_rom_load,
        true,
        #[cfg(target_arch = "wasm32")]
        wasm_event_loop_proxy,
    );
    #[cfg(not(target_arch = "wasm32"))]
    let mut app = app;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((system, rom_path, source_path, rom_hash, supports_symbol_loading)) =
        app.initial_backend.as_ref().map(|b| {
            (
                b.system(),
                b.rom_path().to_path_buf(),
                b.source_path().to_path_buf(),
                b.rom_hash(),
                b.supports_symbol_loading(),
            )
        })
    {
        app.start_symbol_load_for_paths(
            system,
            rom_path,
            source_path,
            rom_hash,
            supports_symbol_loading,
        );
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
    #[cfg(not(target_arch = "wasm32"))]
    emu_worker_generation: u64,
    audio: Option<AudioOutput>,
    gamepad: Option<GamepadHandler>,
    gfx: Option<Graphics>,
    #[cfg(target_arch = "wasm32")]
    pending_gfx: PendingGfx,
    #[cfg(target_arch = "wasm32")]
    pending_rom_load: crate::platform::FileDataSlot,
    #[cfg(target_arch = "wasm32")]
    pending_wasm_rom_after_flush: Option<(String, Vec<u8>)>,
    #[cfg(target_arch = "wasm32")]
    pending_state_load: crate::platform::FileDataSlot,
    #[cfg(target_arch = "wasm32")]
    pending_nes_palette_load: crate::platform::FileDataSlot,
    #[cfg(target_arch = "wasm32")]
    wasm_tab_visible: std::rc::Rc<std::cell::Cell<bool>>,
    #[cfg(target_arch = "wasm32")]
    wasm_tab_was_visible: bool,
    #[cfg(target_arch = "wasm32")]
    wasm_retired_threads: Vec<(EmuThread, bool)>,
    #[cfg(target_arch = "wasm32")]
    wasm_event_loop_proxy: winit::event_loop::EventLoopProxy<()>,
    window_id: Option<WindowId>,
    fps_tracker: FpsTracker,
    debug_windows: DebugWindowState,
    debug_dock: egui_dock::DockState<DebugTab>,
    active_debug_presentation: DebugPresentation,
    exit_requested: bool,
    settings: Settings,
    input_configuration: input_configuration::ScopedInputRuntime,
    timing: TimingState,
    last_audio_output_sample_rate: u32,
    #[cfg(not(target_arch = "wasm32"))]
    last_audio_host_config: (Option<String>, crate::settings::AudioBufferPolicy),
    #[cfg(not(target_arch = "wasm32"))]
    last_audio_recovery: Option<Instant>,
    speed: SpeedState,
    pressed_keyboard_targets: keyboard::PressedKeyboardTargets,
    held_frontend_sources: keyboard::HeldFrontendSources,
    keyboard_capture_active: bool,
    pause_state: pause::PauseState,
    modifiers: ModifierKeys,
    host_input: HostInputState,
    cursor_pos: Option<(f32, f32)>,
    mouse_left_pressed: bool,
    mouse_right_pressed: bool,
    pce_mouse_motion: (f64, f64),
    pce_mouse_captured: bool,
    window_size: (f32, f32),
    tilt: TiltState,
    camera: CameraState,
    last_state_dir: Option<std::path::PathBuf>,
    show_settings_window: bool,
    show_mods_window: bool,
    #[cfg(not(target_arch = "wasm32"))]
    show_cheats_window: bool,
    #[cfg(not(target_arch = "wasm32"))]
    show_audio_explorer: bool,
    show_printer_window: bool,
    debug_requests: DebugRequests,
    active_save_slot: u8,
    latest_frame: Option<Arc<crate::emu_thread::PublishedFramebuffer>>,
    last_core_frame: Option<Arc<crate::emu_thread::PublishedFramebuffer>>,
    last_displayed_frame: Option<Arc<crate::emu_thread::PublishedFramebuffer>>,
    recycled: RecycledBuffers,
    frames_in_flight: usize,
    cached_ui_data: Option<ui::UiFrameData>,
    rom_info: CachedRomInfo,
    symbols: crate::symbols::SymbolSession,
    #[cfg(not(target_arch = "wasm32"))]
    pending_symbol_load: Option<PendingSymbolLoad>,
    #[cfg(not(target_arch = "wasm32"))]
    next_symbol_load_id: u64,
    #[cfg(not(target_arch = "wasm32"))]
    pending_rom_preparation: Option<PendingRomPreparation>,
    #[cfg(not(target_arch = "wasm32"))]
    next_rom_preparation_id: u64,
    #[cfg(not(target_arch = "wasm32"))]
    deferred_initial_rom_load: Option<std::path::PathBuf>,
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
    #[cfg(not(target_arch = "wasm32"))]
    netplay: netplay::Frontend,
    #[cfg(not(target_arch = "wasm32"))]
    tas_control: tas_control::TasControlCoordinator,
    #[cfg(not(target_arch = "wasm32"))]
    tas_editor_live_validation_cache: tas_editor::TasEditorLiveValidationCache,
    #[cfg(not(target_arch = "wasm32"))]
    tas_repair: tas_control::repair::TasRepairManager,
    #[cfg(not(target_arch = "wasm32"))]
    pending_tas_repair_activation: Option<tas_control::repair::TasPreparedRepair>,
    #[cfg(not(target_arch = "wasm32"))]
    pending_tas_autofire: Option<autofire::AutofireState>,
    autofire_state: autofire::AutofireState,
    autofire_rearm_pending: bool,
    autofire_released_targets: [[bool; 8]; 5],
    autofire_legacy_release_pending: bool,
    autofire_observed_buttons: [u8; 5],
    autofire_observed_legacy_held: bool,
    #[cfg(not(target_arch = "wasm32"))]
    tas_realtime_recorder: tas_control::realtime::TasRealtimeRecorder,
    #[cfg(not(target_arch = "wasm32"))]
    tas_playback_scheduler: tas_control::realtime::TasPlaybackScheduler,
    #[cfg(not(target_arch = "wasm32"))]
    tas_verified_replay_export: Option<tas_editor::VerifiedReplayExportCoordinator>,
    window_focused: bool,
    game_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    debugger_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    settings_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    mods_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    cheats_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    audio_explorer_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    printer_window_focused: bool,
    #[cfg(not(target_arch = "wasm32"))]
    focus_settings_window_pending: bool,
    #[cfg(not(target_arch = "wasm32"))]
    focus_mods_window_pending: bool,
    #[cfg(not(target_arch = "wasm32"))]
    focus_cheats_window_pending: bool,
    #[cfg(not(target_arch = "wasm32"))]
    focus_audio_explorer_pending: bool,
    #[cfg(not(target_arch = "wasm32"))]
    focus_printer_window_pending: bool,
    focus_state_dirty: bool,
    #[cfg(not(target_arch = "wasm32"))]
    last_debugger_render: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_settings_render: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_mods_render: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_cheats_render: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_audio_explorer_render: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_printer_render: Instant,
    egui_wants_keyboard: bool,
    game_view_focused: bool,
    active_system: ActiveSystem,
    game_boy_serial_device: zeff_gb_core::hardware::GameBoySerialDevice,
    media_slot_snapshot: Option<zeff_emu_common::media::MediaSlotSnapshot>,
    ws_display_rotated: bool,
    cached_slot_info: state_io::SlotInfo,
    undo_load_state: Option<Vec<u8>>,
    undo_save_state_path: Option<std::path::PathBuf>,
    recovery_state_available: bool,
    suppress_unfocus_pause_until_focus: bool,
}

impl App {
    pub(super) fn core_supports_save_states(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_save_states)
    }

    pub(super) fn core_supports_state_capture(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_state_capture)
    }

    pub(super) fn core_supports_rewind(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_rewind)
    }

    pub(super) fn core_supports_replay(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_replay)
    }

    pub(super) fn core_supports_audio(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_audio)
    }

    pub(super) fn core_supports_cheats(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_cheats)
    }

    pub(super) fn core_supports_guest_calls(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_guest_calls)
    }

    pub(super) fn core_supports_debugger(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_debugger)
    }

    pub(super) fn core_supports_execution_controls(&self) -> bool {
        self.emu_thread
            .as_ref()
            .is_some_and(|thread| thread.capabilities().supports_execution_controls)
    }
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
        #[cfg(not(target_arch = "wasm32"))]
        if self.realtime_tas_recording_active() {
            return "TAS Recording";
        }
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
        let base = std::time::Duration::from_nanos(self.nominal_frame_duration_ns());
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

    fn nominal_frame_duration_ns(&self) -> u64 {
        self.emu_thread.as_ref().map_or_else(
            || self.active_system.frame_duration_ns(),
            EmuThread::nominal_frame_duration_ns,
        )
    }

    fn left_stick_controls_tilt(&self, is_mbc7: bool) -> bool {
        match self.input_configuration.resolved.tilt.left_stick_mode {
            LeftStickMode::Tilt => true,
            LeftStickMode::Dpad | LeftStickMode::BindingsOnly => false,
            LeftStickMode::Auto => is_mbc7,
        }
    }

    fn left_stick_controls_dpad(&self, is_mbc7: bool) -> bool {
        self.input_configuration.resolved.tilt.left_stick_mode != LeftStickMode::BindingsOnly
            && !self.left_stick_controls_tilt(is_mbc7)
    }

    fn sync_host_input_with_stick_mode(&mut self, is_mbc7: bool) {
        if self.left_stick_controls_dpad(is_mbc7) {
            self.host_input.set_gamepad_stick_dpad(
                self.tilt.left_stick,
                self.input_configuration.resolved.tilt.deadzone,
            );
        } else {
            self.host_input.clear_gamepad_stick_dpad();
        }
    }

    fn mouse_tilt_vector(&self) -> (f32, f32) {
        tilt::mouse_tilt_vector(self.cursor_pos, self.window_size)
    }

    fn tilt_config(&self) -> TiltConfig {
        TiltConfig {
            sensitivity: self.input_configuration.resolved.tilt.sensitivity,
            invert_x: self.input_configuration.resolved.tilt.invert_x,
            invert_y: self.input_configuration.resolved.tilt.invert_y,
            deadzone: self.input_configuration.resolved.tilt.deadzone,
            stick_bypass_lerp: self.input_configuration.resolved.tilt.stick_bypass_lerp,
            lerp: self.input_configuration.resolved.tilt.lerp,
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
            self.input_configuration.resolved.tilt.input_mode,
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
        let has_tilt_sensor = self.rom_info.is_mbc7 || self.rom_info.is_gba_tilt;
        let keyboard = self.host_input.tilt_vector();
        let mouse = self.mouse_tilt_vector();
        let left_stick = self.tilt.left_stick;

        self.sync_host_input_with_stick_mode(has_tilt_sensor);
        let target = self.compute_target_tilt(has_tilt_sensor, keyboard, mouse, left_stick);
        self.update_smoothed_tilt(target, has_tilt_sensor)
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

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        self.handle_device_event(event);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        #[cfg(target_arch = "wasm32")]
        {
            self.wasm_poll_hooks(_event_loop);
            if let Some(thread) = &self.emu_thread {
                thread.poll_persistence();
            }
            self.poll_retired_wasm_threads();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.wasm_poll_hooks(event_loop);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.drain_live_control();
            if self.netplay.fenced() {
                self.drain_emu_responses();
                self.pump_netplay();
            }
            self.sync_debug_presentation(event_loop);
            self.sync_settings_window(event_loop);
            self.sync_mods_window(event_loop);
            self.sync_cheats_window(event_loop);
            self.sync_audio_explorer_window(event_loop);
            self.sync_printer_window(event_loop);
            self.sync_tas_editor(event_loop);
            self.sync_netplay_window(event_loop);
            self.redraw_netplay_status(Instant::now());
        }
        self.apply_focus_state();
        self.schedule_next_frame(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.perform_shutdown();
        #[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
        browser_speculation_test::record_app_exiting();
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
