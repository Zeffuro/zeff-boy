use super::*;

pub(super) fn create(
    backend: Option<EmuBackend>,
    settings: Settings,
    #[cfg(not(target_arch = "wasm32"))] deferred_initial_rom_load: Option<std::path::PathBuf>,
    host_devices: bool,
    #[cfg(target_arch = "wasm32")] wasm_event_loop_proxy: winit::event_loop::EventLoopProxy<()>,
) -> App {
    let uncapped_speed = settings.emulation.uncapped_speed;
    let uncapped_frames_per_tick = settings
        .emulation
        .uncapped_frames_per_tick
        .clamp(1, crate::emu_thread::MAX_UNCAPPED_BATCH_SIZE);
    let vsync_mode = settings.video.vsync_mode;
    let initial_audio_output_sample_rate = settings.audio.output_sample_rate;
    #[cfg(not(target_arch = "wasm32"))]
    let initial_audio_host_config = (
        settings.audio.output_device_id.clone(),
        settings.audio.buffer_policy,
    );
    let initial_debug_presentation =
        lifecycle::effective_debug_presentation(settings.ui.debug_presentation);
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

    let cached_is_mbc7 = backend.as_ref().is_some_and(|b| b.is_mbc7());
    let cached_is_gba_tilt = backend.as_ref().is_some_and(|b| b.is_gba_tilt());
    let cached_is_pocket_camera = backend.as_ref().is_some_and(|b| b.is_pocket_camera());
    let cached_rom_path = backend.as_ref().map(|b| b.rom_path().to_path_buf());
    let cached_source_path = backend.as_ref().map(|b| b.source_path().to_path_buf());
    let initial_ws_display_rotated = backend.as_ref().and_then(|b| b.ws()).is_some_and(|ws| {
        ws.preferred_orientation() == zeff_ws_core::hardware::cartridge::RomOrientation::Vertical
    });
    let initial_game_boy_serial_device = backend
        .as_ref()
        .and_then(EmuBackend::game_boy_serial_device)
        .unwrap_or_default();
    let initial_media_slot_snapshot = backend.as_ref().and_then(EmuBackend::media_slot_snapshot);
    let active_system = backend
        .as_ref()
        .map(|b| b.system())
        .unwrap_or(ActiveSystem::GameBoy);

    #[allow(unused_mut)]
    let mut app = App {
        emu_thread: None,
        #[cfg(not(target_arch = "wasm32"))]
        emu_worker_generation: 0,
        initial_backend: backend,
        audio: None,
        gamepad: if host_devices {
            GamepadHandler::new()
                .map_err(|e| log::error!("Gamepad init failed: {e}"))
                .ok()
        } else {
            None
        },
        gfx: None,
        #[cfg(target_arch = "wasm32")]
        pending_gfx: None,
        #[cfg(target_arch = "wasm32")]
        pending_rom_load: std::rc::Rc::new(std::cell::RefCell::new(None)),
        #[cfg(target_arch = "wasm32")]
        pending_wasm_rom_after_flush: None,
        #[cfg(target_arch = "wasm32")]
        browser_netplay_media: None,
        #[cfg(target_arch = "wasm32")]
        pending_state_load: std::rc::Rc::new(std::cell::RefCell::new(None)),
        #[cfg(target_arch = "wasm32")]
        pending_nes_palette_load: std::rc::Rc::new(std::cell::RefCell::new(None)),
        #[cfg(target_arch = "wasm32")]
        wasm_tab_visible: std::rc::Rc::new(std::cell::Cell::new(true)),
        #[cfg(target_arch = "wasm32")]
        wasm_tab_was_visible: true,
        #[cfg(target_arch = "wasm32")]
        wasm_retired_threads: Vec::new(),
        #[cfg(target_arch = "wasm32")]
        wasm_event_loop_proxy,
        window_id: None,
        fps_tracker: FpsTracker::new(),
        debug_windows: DebugWindowState::new(),
        debug_dock: initial_debug_dock,
        active_debug_presentation: initial_debug_presentation,
        exit_requested: false,
        input_configuration: input_configuration::ScopedInputRuntime::new(&settings),
        settings,
        timing: TimingState {
            last_frame_time: Instant::now(),
            last_render_time: Instant::now(),
            last_viewer_update: Instant::now(),
            uncapped_speed,
            uncapped_worker_enabled: false,
            last_uncapped_frames_per_tick: uncapped_frames_per_tick,
            last_vsync_mode: vsync_mode,
            last_speed_mode: SpeedMode::Normal,
        },
        last_audio_output_sample_rate: initial_audio_output_sample_rate,
        #[cfg(not(target_arch = "wasm32"))]
        last_audio_host_config: initial_audio_host_config,
        #[cfg(not(target_arch = "wasm32"))]
        last_audio_recovery: None,
        speed: SpeedState {
            paused: false,
            fast_forward_held: false,
            turbo_held: false,
        },
        pressed_keyboard_targets: Default::default(),
        held_frontend_sources: Default::default(),
        keyboard_capture_active: false,
        pause_state: pause::PauseState::new(),
        modifiers: ModifierKeys::default(),
        host_input: HostInputState::new(),
        cursor_pos: None,
        mouse_left_pressed: false,
        mouse_right_pressed: false,
        pce_mouse_motion: (0.0, 0.0),
        pce_mouse_captured: false,
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
        show_mods_window: false,
        #[cfg(not(target_arch = "wasm32"))]
        show_cheats_window: false,
        #[cfg(not(target_arch = "wasm32"))]
        show_audio_explorer: false,
        show_printer_window: false,
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
            is_gba_tilt: cached_is_gba_tilt,
            is_pocket_camera: cached_is_pocket_camera,
            rom_path: cached_rom_path,
            source_path: cached_source_path,
            rom_hash: None,
            pce_controller_profile_hash: None,
            replay_metadata: None,
        },
        symbols: crate::symbols::SymbolSession::default(),
        #[cfg(not(target_arch = "wasm32"))]
        pending_symbol_load: None,
        #[cfg(not(target_arch = "wasm32"))]
        next_symbol_load_id: 0,
        #[cfg(not(target_arch = "wasm32"))]
        pending_rom_preparation: None,
        #[cfg(not(target_arch = "wasm32"))]
        next_rom_preparation_id: 0,
        #[cfg(not(target_arch = "wasm32"))]
        deferred_initial_rom_load,
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
        #[cfg(not(target_arch = "wasm32"))]
        live_control: crate::live_control::LiveControl::from_env(),
        #[cfg(not(target_arch = "wasm32"))]
        live_button_releases: Vec::new(),
        #[cfg(not(target_arch = "wasm32"))]
        tcp_link_active: false,
        netplay: netplay::Frontend::default(),
        #[cfg(not(target_arch = "wasm32"))]
        tas_control: tas_control::TasControlCoordinator::new(),
        #[cfg(not(target_arch = "wasm32"))]
        tas_editor_live_validation_cache: tas_editor::TasEditorLiveValidationCache::default(),
        #[cfg(not(target_arch = "wasm32"))]
        tas_repair: tas_control::repair::TasRepairManager::new(),
        #[cfg(not(target_arch = "wasm32"))]
        pending_tas_repair_activation: None,
        #[cfg(not(target_arch = "wasm32"))]
        pending_tas_autofire: None,
        autofire_state: autofire::AutofireState::default(),
        autofire_rearm_pending: false,
        autofire_released_targets: [[false; 8]; 5],
        autofire_legacy_release_pending: false,
        autofire_observed_buttons: [0; 5],
        autofire_observed_legacy_held: false,
        #[cfg(not(target_arch = "wasm32"))]
        tas_realtime_recorder: tas_control::realtime::TasRealtimeRecorder::default(),
        #[cfg(not(target_arch = "wasm32"))]
        tas_playback_scheduler: tas_control::realtime::TasPlaybackScheduler::default(),
        #[cfg(not(target_arch = "wasm32"))]
        tas_verified_replay_export: None,
        window_focused: true,
        game_window_focused: true,
        #[cfg(not(target_arch = "wasm32"))]
        debugger_window_focused: false,
        #[cfg(not(target_arch = "wasm32"))]
        settings_window_focused: false,
        #[cfg(not(target_arch = "wasm32"))]
        mods_window_focused: false,
        #[cfg(not(target_arch = "wasm32"))]
        cheats_window_focused: false,
        #[cfg(not(target_arch = "wasm32"))]
        audio_explorer_window_focused: false,
        #[cfg(not(target_arch = "wasm32"))]
        printer_window_focused: false,
        #[cfg(not(target_arch = "wasm32"))]
        focus_settings_window_pending: false,
        #[cfg(not(target_arch = "wasm32"))]
        focus_mods_window_pending: false,
        #[cfg(not(target_arch = "wasm32"))]
        focus_cheats_window_pending: false,
        #[cfg(not(target_arch = "wasm32"))]
        focus_audio_explorer_pending: false,
        #[cfg(not(target_arch = "wasm32"))]
        focus_printer_window_pending: false,
        focus_state_dirty: false,
        #[cfg(not(target_arch = "wasm32"))]
        last_debugger_render: Instant::now(),
        #[cfg(not(target_arch = "wasm32"))]
        last_settings_render: Instant::now(),
        #[cfg(not(target_arch = "wasm32"))]
        last_mods_render: Instant::now(),
        #[cfg(not(target_arch = "wasm32"))]
        last_cheats_render: Instant::now(),
        #[cfg(not(target_arch = "wasm32"))]
        last_audio_explorer_render: Instant::now(),
        #[cfg(not(target_arch = "wasm32"))]
        last_printer_render: Instant::now(),
        egui_wants_keyboard: false,
        game_view_focused: true,
        active_system,
        game_boy_serial_device: initial_game_boy_serial_device,
        media_slot_snapshot: initial_media_slot_snapshot,
        ws_display_rotated: initial_ws_display_rotated,
        cached_slot_info: state_io::SlotInfo {
            labels: std::array::from_fn(|i| format!("Slot {i}  (empty)")),
            occupied: [false; 10],
        },
        undo_load_state: None,
        undo_save_state_path: None,
        recovery_state_available: false,
        suppress_unfocus_pause_until_focus: false,
    };

    app.debug_windows.memory.configure_for_system(active_system);
    app
}
