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
        DebugUiActions, DebugWindowState, FpsTracker, ToastManager, create_default_dock_state,
        create_dock_from_saved_tabs,
    },
    emu_backend::{ActiveSystem, EmuBackend},
    emu_thread::EmuThread,
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

pub(crate) fn run(backend: Option<EmuBackend>, settings: Settings) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let uncapped_speed = settings.uncapped_speed;
    let vsync_mode = settings.vsync_mode;

    // Cache metadata before handing emulator to emu thread
    let cached_is_mbc7 = backend.as_ref().is_some_and(|b| b.is_mbc7());
    let cached_rom_path = backend.as_ref().map(|b| b.rom_path().to_path_buf());
    let active_system = backend.as_ref().map(|b| b.system()).unwrap_or(ActiveSystem::GameBoy);

    let mut app = App {
        emu_thread: None,
        initial_backend: backend,
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
        last_state_dir: None,
        show_settings_window: false,
        debug_requests: DebugRequests::new(),
        active_save_slot: 1,
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

struct DebugRequests {
    step: bool,
    continue_: bool,
    backstep: bool,
    frame_advance: bool,
}

impl DebugRequests {
    fn new() -> Self {
        Self {
            step: false,
            continue_: false,
            backstep: false,
            frame_advance: false,
        }
    }

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
    debug_dock: egui_dock::DockState<crate::debug::DebugTab>,
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
                let multi = self.settings.fast_forward_multiplier.max(1) as u32;
                base / multi
            }
            _ => base,
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
            self.recycled.framebuffer = Some(old);
        }
        self.cached_is_mbc7 = result.is_mbc7;
        self.rewind.fill = result.rewind_fill;

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

        if let Some(recorder) = &mut self.recording.audio_recorder {
            recorder.write_samples(&result.audio_samples);
            if let Some(snapshot) = result.apu_snapshot {
                recorder.write_apu_snapshot(snapshot);
            }
        }
        self.recycled.audio = Some(result.audio_samples);

        let mut ui_data = result.ui_data;

        if let Some(ref mut cached) = self.cached_ui_data {
            if ui_data.graphics_data.is_some() {
                if let Some(old_gfx) = cached.graphics_data.take() {
                    let crate::debug::ConsoleGraphicsData::Gb(gb) = old_gfx;
                    if !gb.vram.is_empty() {
                        self.recycled.vram = Some(gb.vram);
                    }
                }
            } else {
                ui_data.graphics_data = cached.graphics_data.take();
            }
            if ui_data.oam_debug.is_none() {
                ui_data.oam_debug = cached.oam_debug.take();
            }
            if let Some(ref disasm) = ui_data.disassembly_view {
                self.debug_windows.last_disasm_pc = Some(disasm.pc);
            } else {
                ui_data.disassembly_view = cached.disassembly_view.take();
            }
            if ui_data.rom_debug.is_none() {
                ui_data.rom_debug = cached.rom_debug.take();
            }
            if ui_data.memory_page.is_some() {
                if let Some(old_page) = cached.memory_page.take() {
                    self.recycled.memory_page = Some(old_page);
                }
            } else {
                ui_data.memory_page = cached.memory_page.take();
            }
        }

        if let Some(ref mut perf) = ui_data.perf_info {
            perf.fps = if self.settings.show_fps {
                self.fps_tracker.fps()
            } else {
                0.0
            };
            perf.speed_mode_label = self.speed_mode_label().to_string();
            perf.frames_in_flight = self.frames_in_flight;
        }

        if let Some(results) = ui_data.memory_search_results.take() {
            self.debug_windows.memory.search_results = results;
        }

        if let Some(results) = ui_data.rom_search_results.take() {
            self.debug_windows.rom_viewer.search_results = results;
        }
        self.debug_windows.rom_viewer.rom_size = ui_data.rom_size;

        if let Some(crate::debug::ConsoleGraphicsData::Gb(ref gb_data)) = ui_data.graphics_data {
            if self.debug_windows.show_tile_viewer {
                self.debug_windows.tiles.update_dirty_inputs(
                    &gb_data.vram,
                    &gb_data.bg_palette_ram,
                    &gb_data.obj_palette_ram,
                    gb_data.ppu.bgp,
                    gb_data.cgb_mode,
                    gb_data.color_correction,
                    gb_data.color_correction_matrix,
                );
            }
            if self.debug_windows.show_tilemap_viewer {
                self.debug_windows.tilemap.update_dirty_inputs(
                    &gb_data.vram,
                    &gb_data.bg_palette_ram,
                    gb_data.ppu,
                    gb_data.cgb_mode,
                    gb_data.color_correction,
                    gb_data.color_correction_matrix,
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

        // Handle backstep: pop one rewind snapshot and pause
        if std::mem::take(&mut self.debug_requests.backstep)
            && self.settings.rewind_enabled
            && !self.rewind.pending
            && !self.rewind.backstep_pending
        {
            if let Some(thread) = &self.emu_thread {
                thread.send(crate::emu_thread::EmuCommand::Rewind);
                self.rewind.backstep_pending = true;
            }
        }

        if self.rewind.held && self.settings.rewind_enabled {
            self.rewind.throttle += 1;
            let pop_interval = self.settings.rewind_speed.max(1);
            if self.rewind.throttle >= pop_interval
                && !self.rewind.pending
                && !self.rewind.backstep_pending
            {
                self.rewind.throttle = 0;
                if let Some(thread) = &self.emu_thread {
                    thread.send(crate::emu_thread::EmuCommand::Rewind);
                    self.rewind.pending = true;
                }
            }
        } else {
            if self.rewind.throttle > 0 {
                self.timing.last_frame_time = Instant::now();
                self.rewind.throttle = 0;
            }
            self.rewind.pops = 0;
            if self.frames_in_flight < MAX_IN_FLIGHT {
                let now = Instant::now();
                let frames_to_step = if self.paused {
                    self.timing.last_frame_time = now;
                    if std::mem::take(&mut self.debug_requests.frame_advance) {
                        1
                    } else {
                        0
                    }
                } else {
                    self.compute_frames_to_step(now)
                };

                let has_pending =
                    self.debug_requests.has_pending() || self.pending_debug_actions.has_pending();

                if frames_to_step > 0 || has_pending {
                    if let Some(thread) = &self.emu_thread {
                        let want_viewer_update = match self.speed_mode() {
                            SpeedMode::Normal => true,
                            SpeedMode::FastForward | SpeedMode::Uncapped => {
                                let now = Instant::now();
                                if now.duration_since(self.timing.last_viewer_update)
                                    >= VIEWER_UPDATE_INTERVAL
                                {
                                    self.timing.last_viewer_update = now;
                                    true
                                } else {
                                    false
                                }
                            }
                        };

                        let (mut buttons_pressed, dpad_pressed) =
                            if let Some(player) = &mut self.recording.replay_player {
                                if let Some((buttons, dpad)) = player.next_frame() {
                                    (buttons, dpad)
                                } else {
                                    self.toast_manager.info("Replay finished");
                                    self.recording.replay_player = None;
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

                        if self.turbo_held {
                            self.turbo_counter = self.turbo_counter.wrapping_add(1);
                            if self.turbo_counter % 2 == 1 {
                                buttons_pressed = 0;
                            }
                        } else {
                            self.turbo_counter = 0;
                        }

                        if let Some(recorder) = &mut self.recording.replay_recorder {
                            recorder.record_frame(buttons_pressed, dpad_pressed);
                        }

                        let reqs = crate::debug::compute_tab_requirements(&self.debug_dock);
                        let input = crate::emu_thread::FrameInput {
                            frames: frames_to_step,
                            host_tilt: tilt_data.smoothed,
                            buttons_pressed,
                            dpad_pressed,
                            debug_step: std::mem::take(&mut self.debug_requests.step),
                            debug_continue: std::mem::take(&mut self.debug_requests.continue_),
                            apu_capture_enabled: reqs.needs_apu && want_viewer_update,
                            skip_audio: match self.speed_mode() {
                                SpeedMode::Uncapped => true,
                                SpeedMode::FastForward => {
                                    self.settings.mute_audio_during_fast_forward
                                }
                                SpeedMode::Normal => false,
                            },
                            midi_capture_active: self
                                .recording
                                .audio_recorder
                                .as_ref()
                                .is_some_and(|r| r.is_midi()),
                            debug_actions: std::mem::replace(
                                &mut self.pending_debug_actions,
                                DebugUiActions::none(),
                            ),
                            snapshot: crate::emu_thread::SnapshotRequest {
                                want_debug_info: (reqs.needs_debug_info || self.settings.show_fps),
                                want_perf_info: reqs.needs_perf_info || self.settings.show_fps,
                                any_viewer_open: reqs.needs_viewer_data && want_viewer_update,
                                any_vram_viewer_open: reqs.needs_vram && want_viewer_update,
                                show_oam_viewer: reqs.needs_oam && want_viewer_update,
                                show_apu_viewer: reqs.needs_apu && want_viewer_update,
                                show_disassembler: reqs.needs_disassembly && want_viewer_update,
                                show_rom_info: reqs.needs_rom_info && want_viewer_update,
                                show_memory_viewer: reqs.needs_memory_page && want_viewer_update,
                                memory_view_start: self.debug_windows.memory.view_start,
                                show_rom_viewer: reqs.needs_rom_page && want_viewer_update,
                                rom_view_start: self.debug_windows.rom_viewer.view_start,
                                last_disasm_pc: self.debug_windows.last_disasm_pc,
                                memory_search: if self.debug_windows.memory.search_pending {
                                    self.debug_windows.memory.search_pending = false;
                                    crate::debug::memory_viewer::parse_search_query(
                                        &self.debug_windows.memory.search_query,
                                        self.debug_windows.memory.search_mode,
                                    )
                                    .map(|pattern| {
                                        crate::emu_thread::MemorySearchRequest {
                                            pattern,
                                            max_results: self
                                                .debug_windows
                                                .memory
                                                .search_max_results,
                                        }
                                    })
                                } else {
                                    None
                                },
                                rom_search: if self.debug_windows.rom_viewer.search_pending {
                                    self.debug_windows.rom_viewer.search_pending = false;
                                    crate::debug::memory_viewer::parse_search_query(
                                        &self.debug_windows.rom_viewer.search_query,
                                        self.debug_windows.rom_viewer.search_mode,
                                    )
                                    .map(|pattern| {
                                        crate::emu_thread::MemorySearchRequest {
                                            pattern,
                                            max_results: self
                                                .debug_windows
                                                .rom_viewer
                                                .search_max_results,
                                        }
                                    })
                                } else {
                                    None
                                },
                                color_correction: self.settings.color_correction,
                                color_correction_matrix: self.settings.color_correction_matrix,
                            },
                            buffers: crate::emu_thread::ReusableBuffers {
                                framebuffer: self.recycled.framebuffer.take(),
                                audio: self.recycled.audio.take(),
                                vram: self.recycled.vram.take(),
                                oam: self.recycled.oam.take(),
                                memory_page: self.recycled.memory_page.take(),
                            },
                            rewind_enabled: self.settings.rewind_enabled && !self.rewind.held,
                            rewind_seconds: self.settings.rewind_seconds,
                        };
                        if self.debug_windows.cheat.cheats_dirty {
                            self.debug_windows.cheat.cheats_dirty = false;
                            thread.send(crate::emu_thread::EmuCommand::UpdateCheats(
                                crate::cheats::collect_enabled_patches(
                                    &self.debug_windows.cheat.user_codes,
                                    &self.debug_windows.cheat.libretro_codes,
                                ),
                            ));
                        }
                        thread.send(crate::emu_thread::EmuCommand::StepFrames(input));
                        self.frames_in_flight += 1;
                    }

                    if frames_to_step > 0 {
                        self.fps_tracker.tick();
                    }
                }
            }
        }

        let should_render = match self.speed_mode() {
            SpeedMode::Normal => true,
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

        self.update_debug_cache_edges();

        if let Some(frame) = self.latest_frame.take() {
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.upload_framebuffer(&frame);
            }
            self.last_displayed_frame = Some(frame.clone());
            self.recycled.framebuffer = Some(frame);
        }

        crate::debug::sync_show_flags(&mut self.debug_windows, &self.debug_dock);
        self.debug_windows.memory.enable_editing = self.settings.enable_memory_editing;

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
