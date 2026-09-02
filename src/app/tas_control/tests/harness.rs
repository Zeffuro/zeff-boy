use std::path::PathBuf;
use std::time::Duration;

use crate::app::App;
use crate::app::input::HostInputState;
use crate::app::pause::PauseState;
use crate::app::tas_control::realtime::{TasPlaybackScheduler, TasRealtimeRecorder};
use crate::app::tas_control::{
    TasControlCoordinator, repair::TasRepairManager, state::TasControlState,
};
use crate::app::tilt::AutoTiltSource;
use crate::app::types::*;
use crate::debug::{DebugUiActions, DebugWindowState, FpsTracker, ToastManager};
use crate::emu_backend::ActiveSystem;
use crate::emu_thread::EmuThread;
use crate::live_control::{LiveCommand, LiveReply};
use crate::platform::Instant;
use crate::settings::{DebugPresentation, Settings, VsyncMode};

pub(super) fn app_with_worker(
    worker: EmuThread,
    worker_generation: u64,
    system: ActiveSystem,
    rom_path: PathBuf,
) -> App {
    let mut settings = Settings::default();
    settings.audio.output_sample_rate = 48_000;
    settings.emulation.uncapped_speed = false;
    settings.emulation.save_recovery_state = false;
    settings.ui.check_for_updates = false;
    let latest_frame = worker.shared_framebuffer().load_full();
    let now = Instant::now();

    App {
        initial_backend: None,
        emu_thread: Some(worker),
        emu_worker_generation: worker_generation,
        audio: None,
        gamepad: None,
        gfx: None,
        window_id: None,
        fps_tracker: FpsTracker::new(),
        debug_windows: DebugWindowState::new(),
        debug_dock: crate::debug::create_default_dock_state(),
        active_debug_presentation: DebugPresentation::Floating,
        exit_requested: false,
        timing: TimingState {
            last_frame_time: now,
            last_render_time: now,
            last_viewer_update: now,
            uncapped_speed: false,
            last_uncapped_frames_per_tick: 1,
            last_vsync_mode: VsyncMode::On,
            last_speed_mode: SpeedMode::Normal,
        },
        last_audio_output_sample_rate: 48_000,
        settings,
        speed: SpeedState {
            paused: false,
            fast_forward_held: false,
            turbo_held: false,
            turbo_counter: 0,
        },
        pause_state: PauseState::new(),
        modifiers: ModifierKeys::default(),
        host_input: HostInputState::new(),
        cursor_pos: None,
        mouse_left_pressed: false,
        mouse_right_pressed: false,
        pce_mouse_motion: (0.0, 0.0),
        pce_mouse_captured: false,
        window_size: (256.0, 240.0),
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
        show_mods_window: false,
        show_cheats_window: false,
        show_printer_window: false,
        debug_requests: DebugRequests::default(),
        active_save_slot: 0,
        latest_frame,
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
            is_mbc7: false,
            is_pocket_camera: false,
            rom_path: Some(rom_path.clone()),
            source_path: Some(rom_path),
            rom_hash: None,
            pce_controller_profile_hash: None,
            replay_metadata: None,
        },
        symbols: crate::symbols::SymbolSession::default(),
        pending_symbol_load: None,
        next_symbol_load_id: 0,
        pending_rom_preparation: None,
        next_rom_preparation_id: 0,
        deferred_initial_rom_load: None,
        nes_palette_cache: NesPaletteFileCache::default(),
        pending_archive_selection: None,
        pending_debug_actions: DebugUiActions::none(),
        shutdown_performed: false,
        toast_manager: ToastManager::new(),
        update_checker: crate::update::UpdateChecker::new(false, None),
        recording: RecordingState {
            audio_recorder: None,
            replay_recorder: None,
            pending_replay_start: None,
            next_replay_capture_id: 0,
            replay_finalization: None,
            replay_player: None,
            pending_replay_batches: std::collections::VecDeque::new(),
            queued_replay_playback_frames: 0,
            replay_recording_origin: ReplayCaptureOrigin::default(),
            replay_media_events_pending: 0,
            pending_media_commands: std::collections::VecDeque::new(),
            last_replay_checkpoint_frame: 0,
            pending_replay_checkpoint_hashes: std::collections::BTreeMap::new(),
        },
        rewind: RewindState {
            held: false,
            fill: 0.0,
            frames_rewound: 0,
            pending: false,
            backstep_pending: false,
            pacer: RewindPacer::default(),
            pace_updated_at: None,
            scheduled_frames: 0,
            active_mode: None,
        },
        remote_debug_frames_remaining: 0,
        remote_memory_view_start: None,
        remote_memory_frames_remaining: 0,
        remote_graphics_frames_remaining: 0,
        remote_zapper: None,
        live_control: crate::live_control::LiveControl::disabled_for_test(),
        live_button_releases: Vec::new(),
        tcp_link_active: false,
        tas_control: TasControlCoordinator::new(),
        tas_repair: TasRepairManager::new(),
        tas_realtime_recorder: TasRealtimeRecorder::default(),
        tas_playback_scheduler: TasPlaybackScheduler::default(),
        tas_verified_replay_export: None,
        window_focused: true,
        game_window_focused: true,
        debugger_window_focused: false,
        settings_window_focused: false,
        mods_window_focused: false,
        cheats_window_focused: false,
        printer_window_focused: false,
        focus_settings_window_pending: false,
        focus_mods_window_pending: false,
        focus_cheats_window_pending: false,
        focus_printer_window_pending: false,
        focus_state_dirty: false,
        last_debugger_render: now,
        last_settings_render: now,
        last_mods_render: now,
        last_cheats_render: now,
        last_printer_render: now,
        egui_wants_keyboard: false,
        game_view_focused: true,
        active_system: system,
        game_boy_serial_device: zeff_gb_core::hardware::GameBoySerialDevice::Disconnected,
        media_slot_snapshot: None,
        ws_display_rotated: false,
        cached_slot_info: crate::app::state_io::SlotInfo {
            labels: std::array::from_fn(|index| format!("Slot {index}  (empty)")),
            occupied: [false; 10],
        },
        undo_load_state: None,
        undo_save_state_path: None,
        recovery_state_available: false,
        suppress_unfocus_pause_until_focus: false,
    }
}

pub(super) fn live_ok(app: &mut App, command: LiveCommand) -> serde_json::Value {
    match app.handle_live_command_for_test(command) {
        LiveReply::Ok(value) => value,
        LiveReply::Error(error) => panic!("live command failed: {error}"),
    }
}

pub(super) fn wait_for_linked(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision { .. }
    ) && Instant::now() < deadline
    {
        app.begin_queued_tas_control_acquire();
        app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 0,
            ..
        }
    ));
}

pub(super) fn wait_for_recorded_frame(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            ..
        }
    ) && Instant::now() < deadline
    {
        app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision {
            candidate_executed_project_frames: 1,
            ..
        }
    ));
}
