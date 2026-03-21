use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::WindowId,
};

use crate::{
    audio::AudioOutput,
    debug::{
        DebugViewerData, DebugWindowState, DisassemblyView, FpsTracker, RomInfoViewData,
        disassemble_around,
    },
    emu_thread::{EmuResponse, EmuThread},
    emulator::Emulator,
    graphics,
    graphics::Graphics,
    hardware::joypad::JoypadKey,
    hardware::types::CPUState,
    hardware::types::hardware_mode::HardwareMode,
    input::GamepadHandler,
    settings::Settings,
};

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
        show_settings_window: false,
        debug_step_requested: false,
        debug_continue_requested: false,
        latest_frame: None,
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
    show_settings_window: bool,
    debug_step_requested: bool,
    debug_continue_requested: bool,
    latest_frame: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeedMode {
    Normal,
    Uncapped,
    FastForward,
}

const GB_FRAME_DURATION: Duration = Duration::from_nanos(16_742_706);

impl App {
    fn ensure_emu_thread(&mut self) {
        if self.emu_thread.is_some() {
            return;
        }
        if let Some(emu) = self.emulator.as_ref() {
            self.emu_thread = Some(EmuThread::spawn(Arc::clone(emu)));
        }
    }

    fn stop_emu_thread(&mut self) {
        if let Some(mut thread) = self.emu_thread.take() {
            thread.shutdown();
        }
    }

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

    fn map_key(&self, key: KeyCode) -> Option<JoypadKey> {
        let keys = &self.settings.key_bindings;
        if key == keys.right {
            Some(JoypadKey::Right)
        } else if key == keys.left {
            Some(JoypadKey::Left)
        } else if key == keys.up {
            Some(JoypadKey::Up)
        } else if key == keys.down {
            Some(JoypadKey::Down)
        } else if key == keys.a {
            Some(JoypadKey::A)
        } else if key == keys.b {
            Some(JoypadKey::B)
        } else if key == keys.start {
            Some(JoypadKey::Start)
        } else if key == keys.select {
            Some(JoypadKey::Select)
        } else {
            None
        }
    }

    fn handle_keyboard_input(&mut self, key_event: &KeyEvent) {
        let PhysicalKey::Code(key_code) = key_event.physical_key else {
            return;
        };

        if let Some(action) = self.debug_windows.rebinding_action {
            if key_event.state == ElementState::Pressed && !key_event.repeat {
                self.settings.key_bindings.set(action, key_code);
                self.debug_windows.rebinding_action = None;
            }
            return;
        }

        match key_code {
            KeyCode::Tab => {
                match key_event.state {
                    ElementState::Pressed if !key_event.repeat => self.fast_forward_held = true,
                    ElementState::Released => self.fast_forward_held = false,
                    _ => {}
                }
                return;
            }
            KeyCode::F11 => {
                if key_event.state == ElementState::Pressed && !key_event.repeat {
                    self.uncapped_speed = !self.uncapped_speed;
                    self.settings.uncapped_speed = self.uncapped_speed;
                    self.settings.save();
                    if let Some(gfx) = self.gfx.as_mut() {
                        gfx.set_uncapped_present_mode(self.uncapped_speed);
                    }
                }
                return;
            }
            _ => {}
        }

        let Some(gb_key) = self.map_key(key_code) else {
            return;
        };

        let Some(emu) = self.emulator.as_ref() else {
            return;
        };
        let mut emu = emu.lock().expect("emulator mutex poisoned");

        match key_event.state {
            ElementState::Pressed => {
                if key_event.repeat {
                    return;
                }
                if emu.bus.io.joypad.key_down(gb_key) {
                    emu.bus.if_reg |= 0x10;
                }
            }
            ElementState::Released => emu.bus.io.joypad.key_up(gb_key),
        }
    }

    fn load_rom(&mut self, path: &std::path::Path) {
        match Emulator::from_rom_with_mode(path, self.settings.hardware_mode_preference) {
            Ok(mut emu) => {
                if let Some(audio) = &self.audio {
                    emu.bus.io.apu.set_sample_rate(audio.sample_rate());
                }
                log::info!("Loaded ROM: {}", path.display());
                self.stop_emu_thread();
                self.emulator = Some(Arc::new(Mutex::new(emu)));
                self.ensure_emu_thread();
                self.fps_tracker = FpsTracker::new();
                self.last_frame_time = Instant::now();
            }
            Err(e) => {
                log::error!("Failed to load ROM '{}': {}", path.display(), e);
            }
        }
    }

    fn open_file_dialog(&mut self) {
        let file = rfd::FileDialog::new()
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .add_filter("All files", &["*"])
            .set_title("Open ROM")
            .pick_file();

        if let Some(path) = file {
            self.load_rom(&path);
        }
    }

    fn handle_dropped_file(&mut self, path: PathBuf) {
        self.load_rom(&path);
    }

    fn tick(&mut self) {
        if self.uncapped_speed != self.settings.uncapped_speed {
            self.uncapped_speed = self.settings.uncapped_speed;
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_uncapped_present_mode(self.uncapped_speed);
            }
        }

        if let (Some(gamepad), Some(emu)) = (&mut self.gamepad, &self.emulator) {
            let mut emu = emu.lock().expect("emulator mutex poisoned");
            for (key, pressed) in gamepad.poll() {
                if pressed {
                    if emu.bus.io.joypad.key_down(key) {
                        emu.bus.if_reg |= 0x10;
                    }
                } else {
                    emu.bus.io.joypad.key_up(key);
                }
            }
        }

        let now = Instant::now();

        let frames_to_step = match self.speed_mode() {
            SpeedMode::FastForward => {
                self.last_frame_time = now;
                self.settings.fast_forward_multiplier
            }
            SpeedMode::Uncapped => {
                self.last_frame_time = now;
                self.settings.uncapped_frames_per_tick
            }
            SpeedMode::Normal => {
                let mut frames = 0;
                while self.last_frame_time + GB_FRAME_DURATION <= now {
                    frames += 1;
                    self.last_frame_time += GB_FRAME_DURATION;
                    if frames > 3 {
                        self.last_frame_time = now;
                        break;
                    }
                }
                frames
            }
        };

        if let Some(emu) = &self.emulator {
            let mut emu = emu.lock().expect("emulator mutex poisoned");
            emu.bus.io.apu.debug_capture_enabled = self.debug_windows.show_apu_viewer;

            if matches!(emu.cpu.running, CPUState::Suspended) {
                if self.debug_continue_requested {
                    emu.debug.clear_hits();
                    emu.debug.break_on_next = false;
                    emu.cpu.running = CPUState::Running;
                    self.debug_continue_requested = false;
                } else if self.debug_step_requested {
                    emu.debug.clear_hits();
                    emu.debug.break_on_next = true;
                    emu.cpu.running = CPUState::Running;
                    self.debug_step_requested = false;
                }
            }
        }

        if frames_to_step > 0 {
            if let Some(thread) = &self.emu_thread {
                thread.send_step_frames(frames_to_step);
            }
        }

        if let Some(thread) = &self.emu_thread {
            let fast_forward_active = matches!(self.speed_mode(), SpeedMode::FastForward);
            while let Some(response) = thread.try_recv() {
                match response {
                    EmuResponse::FrameReady(frame) => self.latest_frame = Some(frame),
                    EmuResponse::AudioSamples(samples) => {
                        if let Some(audio) = &self.audio {
                            audio.queue_samples(
                                &samples,
                                self.settings.master_volume,
                                fast_forward_active,
                                self.settings.mute_audio_during_fast_forward,
                            );
                        }
                    }
                }
            }
        }

        if frames_to_step > 0 {
            self.fps_tracker.tick();
        }

        if let Some(frame) = self.latest_frame.take() {
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.upload_framebuffer(&frame);
            }
        }

        let any_viewer_open = self.debug_windows.any_viewer_open();
        let any_vram_viewer_open = self.debug_windows.any_vram_viewer_open();
        let show_apu_viewer = self.debug_windows.show_apu_viewer;
        let show_disassembler = self.debug_windows.show_disassembler;
        let show_rom_info = self.debug_windows.show_rom_info;
        let show_memory_viewer = self.debug_windows.show_memory_viewer;

        let (debug_info, viewer_data, disassembly_view, rom_info_view, memory_page) =
            if let Some(emu) = self.emulator.as_ref() {
                let emu = emu.lock().expect("emulator mutex poisoned");

                let debug_info = {
                    let mut info = emu.snapshot();
                    info.fps = if self.settings.show_fps {
                        self.fps_tracker.fps()
                    } else {
                        0.0
                    };
                    info.speed_mode_label = self.speed_mode_label();
                    Some(info)
                };

                let viewer_data = if any_viewer_open {
                    Some(DebugViewerData {
                        vram: if any_vram_viewer_open {
                            emu.vram().to_vec()
                        } else {
                            Vec::new()
                        },
                        oam: emu.oam().to_vec(),
                        apu_regs: if show_apu_viewer {
                            emu.bus.io.apu.regs_snapshot()
                        } else {
                            [0; 0x17]
                        },
                        apu_wave_ram: if show_apu_viewer {
                            emu.bus.io.apu.wave_ram_snapshot()
                        } else {
                            [0; 0x10]
                        },
                        apu_nr52: if show_apu_viewer {
                            emu.bus.io.apu.nr52_raw()
                        } else {
                            0
                        },
                        apu_channel_samples: if show_apu_viewer {
                            [
                                emu.bus.io.apu.channel_debug_samples_ordered(0),
                                emu.bus.io.apu.channel_debug_samples_ordered(1),
                                emu.bus.io.apu.channel_debug_samples_ordered(2),
                                emu.bus.io.apu.channel_debug_samples_ordered(3),
                            ]
                        } else {
                            [[0.0; 512]; 4]
                        },
                        apu_master_samples: if show_apu_viewer {
                            emu.bus.io.apu.master_debug_samples_ordered()
                        } else {
                            [0.0; 512]
                        },
                        apu_channel_muted: if show_apu_viewer {
                            emu.bus.io.apu.channel_mutes()
                        } else {
                            [false; 4]
                        },
                        ppu: emu.ppu_registers(),
                        cgb_mode: matches!(
                            emu.hardware_mode,
                            HardwareMode::CGBNormal | HardwareMode::CGBDouble
                        ),
                        bg_palette_ram: emu.bus.io.ppu.bg_palette_ram,
                        obj_palette_ram: emu.bus.io.ppu.obj_palette_ram,
                    })
                } else {
                    None
                };

                let disassembly_view = if show_disassembler {
                    let mut breakpoints: Vec<u16> = emu.debug.breakpoints.iter().copied().collect();
                    breakpoints.sort_unstable();
                    Some(DisassemblyView {
                        pc: emu.cpu.pc,
                        lines: disassemble_around(
                            |addr| emu.bus.read_byte(addr),
                            emu.cpu.pc,
                            12,
                            26,
                        ),
                        breakpoints,
                    })
                } else {
                    None
                };

                let rom_info_view = if show_rom_info {
                    let header = emu.rom_info();
                    let rom_bytes = emu.bus.cartridge.rom_bytes();
                    let manufacturer = header
                        .manufacturer_code
                        .as_deref()
                        .unwrap_or("N/A")
                        .to_string();
                    Some(RomInfoViewData {
                        title: header.title.clone(),
                        manufacturer,
                        publisher: header.publisher().to_string(),
                        cartridge_type: format!("{:?}", header.cartridge_type),
                        rom_size: format!("{:?}", header.rom_size),
                        ram_size: format!("{:?}", header.ram_size),
                        cgb_flag: header.cgb_flag,
                        sgb_flag: header.sgb_flag,
                        is_cgb_compatible: header.is_cgb_compatible,
                        is_cgb_exclusive: header.is_cgb_exclusive,
                        is_sgb_supported: header.is_sgb_supported,
                        header_checksum_valid: header.verify_header_checksum(rom_bytes),
                        global_checksum_valid: header.verify_global_checksum(rom_bytes),
                        hardware_mode: emu.hardware_mode,
                        cartridge_state: emu.cartridge_state(),
                    })
                } else {
                    None
                };

                let memory_page = if show_memory_viewer {
                    Some(emu.read_memory_range(self.debug_windows.memory_view_start, 256))
                } else {
                    None
                };

                (
                    debug_info,
                    viewer_data,
                    disassembly_view,
                    rom_info_view,
                    memory_page,
                )
            } else {
                (None, None, None, None, None)
            };

        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };

        let previous_settings = self.settings.clone();
        match gfx.render(
            debug_info.as_ref(),
            viewer_data.as_ref(),
            rom_info_view.as_ref(),
            disassembly_view.as_ref(),
            memory_page.as_deref(),
            &mut self.debug_windows,
            &mut self.settings,
            &mut self.show_settings_window,
        ) {
            Ok(result) => {
                if result.open_file_requested {
                    self.open_file_dialog();
                }
                if let Some(emu) = self.emulator.as_ref() {
                    let mut emu = emu.lock().expect("emulator mutex poisoned");
                    if let Some(addr) = result.debug_actions.add_breakpoint {
                        emu.debug.add_breakpoint(addr);
                    }
                    if let Some((addr, watch_type)) = result.debug_actions.add_watchpoint {
                        emu.debug.add_watchpoint(addr, watch_type);
                    }
                    for addr in &result.debug_actions.remove_breakpoints {
                        emu.debug.remove_breakpoint(*addr);
                    }
                    for addr in &result.debug_actions.toggle_breakpoints {
                        emu.debug.toggle_breakpoint(*addr);
                    }
                    if let Some(mutes) = result.debug_actions.apu_channel_mutes {
                        emu.bus.io.apu.set_channel_mutes(mutes);
                    }
                    #[cfg(debug_assertions)]
                    for (addr, value) in &result.debug_actions.memory_writes {
                        emu.bus.write_byte(*addr, *value);
                    }
                }
                if result.debug_actions.step_requested {
                    self.debug_step_requested = true;
                }
                if result.debug_actions.continue_requested {
                    self.debug_continue_requested = true;
                }
                if !self.show_settings_window {
                    self.debug_windows.rebinding_action = None;
                }
            }
            Err(graphics::FrameError::Outdated) | Err(graphics::FrameError::Lost) => {
                let size = gfx.size();
                gfx.resize(size.width, size.height);
            }
            Err(graphics::FrameError::Timeout)
            | Err(graphics::FrameError::Occluded)
            | Err(graphics::FrameError::Validation) => {}
            Err(graphics::FrameError::OutOfMemory) => self.exit_requested = true,
        }

        if self.settings != previous_settings {
            self.settings.save();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }

        if self.audio.is_none() {
            self.audio = AudioOutput::new();
            if let (Some(audio), Some(emu)) = (self.audio.as_ref(), self.emulator.as_ref()) {
                let mut emu = emu.lock().expect("emulator mutex poisoned");
                emu.bus.io.apu.set_sample_rate(audio.sample_rate());
            }
        }

        self.ensure_emu_thread();

        let mut gfx =
            pollster::block_on(Graphics::new(event_loop)).expect("failed to initialize graphics");
        gfx.set_uncapped_present_mode(self.uncapped_speed);
        self.window_id = Some(gfx.window().id());
        self.gfx = Some(gfx);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.gfx.is_none() {
            return;
        }
        if Some(window_id) != self.window_id {
            return;
        }

        if let WindowEvent::KeyboardInput { event, .. } = &event {
            self.handle_keyboard_input(event);
        }

        {
            let gfx = self.gfx.as_mut().expect("graphics initialized");
            if gfx.handle_event(&event) {
                return;
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                self.stop_emu_thread();
                event_loop.exit()
            }
            WindowEvent::Resized(size) => {
                let gfx = self.gfx.as_mut().expect("graphics initialized");
                gfx.resize(size.width, size.height)
            }
            WindowEvent::DroppedFile(path) => self.handle_dropped_file(path),
            WindowEvent::RedrawRequested => self.tick(),

            _ => {}
        }

        if self.exit_requested {
            self.stop_emu_thread();
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            match self.speed_mode() {
                SpeedMode::Normal => {
                    let now = Instant::now();
                    let next_frame_time = self.last_frame_time + GB_FRAME_DURATION;
                    if now >= next_frame_time {
                        event_loop.set_control_flow(ControlFlow::Poll);
                        gfx.window().request_redraw();
                    } else {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame_time));
                    }
                }
                SpeedMode::Uncapped | SpeedMode::FastForward => {
                    event_loop.set_control_flow(ControlFlow::Poll);
                    gfx.window().request_redraw();
                }
            }
        }
    }
}
