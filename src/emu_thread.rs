use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{self as chan, Receiver, Sender, TrySendError};

use crate::debug::DebugUiActions;
use crate::emulator::Emulator;
use crate::hardware::types::CPUState;
use crate::ui;

pub(crate) struct SnapshotRequest {
    pub(crate) want_debug_info: bool,
    pub(crate) any_viewer_open: bool,
    pub(crate) any_vram_viewer_open: bool,
    pub(crate) show_oam_viewer: bool,
    pub(crate) show_apu_viewer: bool,
    pub(crate) show_disassembler: bool,
    pub(crate) show_rom_info: bool,
    pub(crate) show_memory_viewer: bool,
    pub(crate) memory_view_start: u16,
    pub(crate) show_rom_viewer: bool,
    pub(crate) rom_view_start: u32,
    pub(crate) last_disasm_pc: Option<u16>,
    pub(crate) memory_search: Option<MemorySearchRequest>,
    pub(crate) rom_search: Option<MemorySearchRequest>,
}

pub(crate) struct MemorySearchRequest {
    pub(crate) pattern: Vec<u8>,
    pub(crate) max_results: usize,
}

pub(crate) struct FrameInput {
    pub(crate) frames: usize,
    pub(crate) host_tilt: (f32, f32),
    pub(crate) buttons_pressed: u8,
    pub(crate) dpad_pressed: u8,
    pub(crate) debug_step: bool,
    pub(crate) debug_continue: bool,
    pub(crate) apu_capture_enabled: bool,
    pub(crate) skip_audio: bool,
    pub(crate) debug_actions: DebugUiActions,
    pub(crate) snapshot: SnapshotRequest,
    pub(crate) reusable_framebuffer: Option<Vec<u8>>,
    pub(crate) reusable_audio_buffer: Option<Vec<f32>>,
    pub(crate) reusable_vram_buffer: Option<Vec<u8>>,
    pub(crate) reusable_oam_buffer: Option<Vec<u8>>,
    pub(crate) reusable_memory_page: Option<Vec<(u16, u8)>>,
    pub(crate) cheats: Vec<crate::cheats::CheatPatch>,
    pub(crate) rewind_enabled: bool,
    pub(crate) rewind_seconds: usize,
}

pub(crate) struct FrameResult {
    pub(crate) frame: Vec<u8>,
    pub(crate) rumble: bool,
    pub(crate) audio_samples: Vec<f32>,
    pub(crate) ui_data: ui::UiFrameData,
    pub(crate) is_mbc7: bool,
    pub(crate) rewind_fill: f32,
}

pub(crate) enum EmuCommand {
    StepFrames(FrameInput),
    SaveStateSlot(u8),
    LoadStateSlot {
        slot: u8,
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    SaveStateToPath(PathBuf),
    LoadStateFromPath {
        path: PathBuf,
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    AutoSaveState,
    AutoLoadState {
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    CaptureStateBytes,
    LoadStateBytes {
        state_bytes: Vec<u8>,
        buttons_pressed: u8,
        dpad_pressed: u8,
    },
    SetSampleRate(u32),
    SetUncapped(bool),
    Rewind,
    Shutdown,
}

pub(crate) enum EmuResponse {
    SaveStateOk(String),
    SaveStateFailed(String),
    LoadStateOk {
        path: String,
        framebuffer: Vec<u8>,
    },
    LoadStateFailed(String),
    StateCaptured(Vec<u8>),
    StateCaptureFailed(String),
    SramFlushed(Option<String>),
    ShutdownComplete,
}

pub(crate) struct EmuThread {
    cmd_tx: Sender<EmuCommand>,
    frame_rx: Receiver<FrameResult>,
    resp_rx: Receiver<EmuResponse>,
    join: Option<JoinHandle<()>>,
}

impl EmuThread {
    pub(crate) fn spawn(mut emu: Emulator) -> Self {
        let (cmd_tx, cmd_rx) = chan::unbounded();
        let (frame_tx, frame_rx) = chan::bounded::<FrameResult>(1);
        let (resp_tx, resp_rx) = chan::unbounded();

        let drain_rx = frame_rx.clone();

        let join = thread::spawn(move || {
            let mut cached_rom_info: Option<crate::debug::RomInfoViewData> = None;
            let mut uncapped_mode = false;
            let mut uncapped_fb: Option<Vec<u8>> = None;
            let mut last_cheats: Vec<crate::cheats::CheatPatch> = Vec::new();

            let mut rewind_buffer = crate::rewind::RewindBuffer::new(10, 4);
            let mut rewind_seconds = 10usize;

            let send_resp = |resp: EmuResponse| -> bool { resp_tx.send(resp).is_ok() };

            'main: loop {
                let command = if uncapped_mode {
                    match cmd_rx.try_recv() {
                        Ok(cmd) => Some(cmd),
                        Err(crossbeam_channel::TryRecvError::Empty) => None,
                        Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                    }
                } else {
                    match cmd_rx.recv() {
                        Ok(cmd) => Some(cmd),
                        Err(_) => break,
                    }
                };

                if let Some(command) = command {
                match command {
                    EmuCommand::SetUncapped(on) => {
                        uncapped_mode = on;
                        emu.bus.io.apu.sample_generation_enabled = !on;
                    }

                    EmuCommand::StepFrames(input) => {
                        last_cheats.clear();
                        last_cheats.extend_from_slice(&input.cheats);

                        Self::install_rom_patches(&mut emu, &input.cheats);

                        Self::apply_debug_actions(&mut emu, &input.debug_actions);

                        if emu
                            .bus
                            .io
                            .joypad
                            .apply_pressed_masks(input.buttons_pressed, input.dpad_pressed)
                        {
                            emu.bus.if_reg |= 0x10;
                        }
                        emu.set_mbc7_host_tilt(input.host_tilt.0, input.host_tilt.1);
                        emu.bus.io.apu.debug_capture_enabled = input.apu_capture_enabled;
                        if !uncapped_mode {
                            emu.bus.io.apu.sample_generation_enabled = !input.skip_audio;
                        }
                        emu.opcode_log.enabled = input.snapshot.want_debug_info;

                        if matches!(emu.cpu.running, CPUState::Suspended) {
                            if input.debug_continue {
                                emu.debug.clear_hits();
                                emu.debug.break_on_next = false;
                                emu.cpu.running = CPUState::Running;
                            } else if input.debug_step {
                                emu.debug.clear_hits();
                                emu.debug.break_on_next = true;
                                emu.cpu.running = CPUState::Running;
                            }
                        }

                        if input.frames > 0 && !matches!(emu.cpu.running, CPUState::Suspended) {
                            for _ in 0..input.frames {
                                emu.step_frame();
                                // Apply RAM cheat patches after each frame
                                Self::apply_ram_cheats(&mut emu, &input.cheats);
                                if matches!(emu.cpu.running, CPUState::Suspended) {
                                    break;
                                }
                            }
                        }

                        if input.rewind_seconds != rewind_seconds {
                            rewind_seconds = input.rewind_seconds;
                            rewind_buffer = crate::rewind::RewindBuffer::new(rewind_seconds, 4);
                        }

                        if input.rewind_enabled && rewind_buffer.tick() {
                            if let Ok(bytes) = crate::save_state::encode_state_bytes(
                                &crate::save_state::SaveStateRef {
                                    version: crate::save_state::SAVE_STATE_VERSION,
                                    rom_hash: emu.rom_hash,
                                    cpu: &emu.cpu,
                                    bus: emu.bus.as_ref(),
                                    hardware_mode_preference: emu.hardware_mode_preference,
                                    hardware_mode: emu.hardware_mode,
                                    cycle_count: emu.cycle_count,
                                    last_opcode: emu.last_opcode,
                                    last_opcode_pc: emu.last_opcode_pc,
                                },
                            ) {
                                rewind_buffer.push(&bytes, emu.framebuffer());
                            }
                        }

                        let ui_data = {
                            if cached_rom_info.is_none() && input.snapshot.show_rom_info {
                                cached_rom_info = Some(ui::compute_static_rom_info(&emu));
                            }
                            ui::collect_emu_snapshot(
                                &emu,
                                &input.snapshot,
                                &cached_rom_info,
                                input.reusable_vram_buffer,
                                input.reusable_oam_buffer,
                                input.reusable_memory_page,
                            )
                        };

                        let src = emu.framebuffer();
                        let mut frame = input.reusable_framebuffer.unwrap_or_default();
                        frame.resize(src.len(), 0);
                        frame.copy_from_slice(src);

                        let rumble = emu.bus.cartridge.rumble_active();
                        let audio_samples = if let Some(mut buf) = input.reusable_audio_buffer {
                            emu.bus.io.apu.drain_samples_into(&mut buf);
                            buf
                        } else {
                            emu.bus.io.apu.drain_samples()
                        };
                        let is_mbc7 = emu.is_mbc7_cartridge();

                        let result = FrameResult {
                            frame,
                            rumble,
                            audio_samples,
                            ui_data,
                            is_mbc7,
                            rewind_fill: rewind_buffer.fill_ratio(),
                        };

                        match frame_tx.try_send(result) {
                            Ok(()) => {}
                            Err(TrySendError::Full(result)) => {
                                let _ = drain_rx.try_recv(); // drop stale frame
                                if frame_tx.try_send(result).is_err() {
                                    break 'main; // disconnected
                                }
                            }
                            Err(TrySendError::Disconnected(_)) => break 'main,
                        }
                    }

                    EmuCommand::SaveStateSlot(slot) => {
                        let resp = match emu.save_state(slot) {
                            Ok(path) => EmuResponse::SaveStateOk(path),
                            Err(err) => EmuResponse::SaveStateFailed(err.to_string()),
                        };
                        if !send_resp(resp) { break 'main; }
                    }

                    EmuCommand::LoadStateSlot {
                        slot,
                        buttons_pressed,
                        dpad_pressed,
                    } => {
                        let resp = match emu.load_state(slot) {
                            Ok(path) => {
                                emu.bus
                                    .io
                                    .joypad
                                    .apply_pressed_masks(buttons_pressed, dpad_pressed);
                                let fb = emu.framebuffer().to_vec();
                                EmuResponse::LoadStateOk {
                                    path,
                                    framebuffer: fb,
                                }
                            }
                            Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
                        };
                        if !send_resp(resp) { break 'main; }
                    }

                    EmuCommand::SaveStateToPath(path) => {
                        let resp = match emu.save_state_to_path(&path) {
                            Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                            Err(err) => EmuResponse::SaveStateFailed(err.to_string()),
                        };
                        if !send_resp(resp) { break 'main; }
                    }

                    EmuCommand::LoadStateFromPath {
                        path,
                        buttons_pressed,
                        dpad_pressed,
                    } => {
                        let resp = match emu.load_state_from_path(&path) {
                            Ok(()) => {
                                emu.bus
                                    .io
                                    .joypad
                                    .apply_pressed_masks(buttons_pressed, dpad_pressed);
                                let fb = emu.framebuffer().to_vec();
                                EmuResponse::LoadStateOk {
                                    path: path.display().to_string(),
                                    framebuffer: fb,
                                }
                            }
                            Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
                        };
                        if !send_resp(resp) { break 'main; }
                    }

                    EmuCommand::SetSampleRate(rate) => {
                        emu.bus.io.apu.set_sample_rate(rate);
                    }


                    EmuCommand::CaptureStateBytes => {
                        let state = crate::save_state::SaveStateRef {
                            version: crate::save_state::SAVE_STATE_VERSION,
                            rom_hash: emu.rom_hash,
                            cpu: &emu.cpu,
                            bus: emu.bus.as_ref(),
                            hardware_mode_preference: emu.hardware_mode_preference,
                            hardware_mode: emu.hardware_mode,
                            cycle_count: emu.cycle_count,
                            last_opcode: emu.last_opcode,
                            last_opcode_pc: emu.last_opcode_pc,
                        };
                        let resp = match crate::save_state::encode_state_bytes(&state) {
                            Ok(bytes) => EmuResponse::StateCaptured(bytes),
                            Err(err) => EmuResponse::StateCaptureFailed(err.to_string()),
                        };
                        if !send_resp(resp) { break 'main; }
                    }

                    EmuCommand::LoadStateBytes {
                        state_bytes,
                        buttons_pressed,
                        dpad_pressed,
                    } => {
                        let resp = match emu.load_state_from_bytes(state_bytes) {
                            Ok(()) => {
                                emu.bus
                                    .io
                                    .joypad
                                    .apply_pressed_masks(buttons_pressed, dpad_pressed);
                                let fb = emu.framebuffer().to_vec();
                                EmuResponse::LoadStateOk {
                                    path: "(replay)".to_string(),
                                    framebuffer: fb,
                                }
                            }
                            Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
                        };
                        if !send_resp(resp) { break 'main; }
                    }

                    EmuCommand::AutoSaveState => {
                        let path = crate::save_state::auto_save_path(emu.rom_hash);
                        let resp = match emu.save_state_to_path(&path) {
                            Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                            Err(err) => EmuResponse::SaveStateFailed(err.to_string()),
                        };
                        if !send_resp(resp) { break 'main; }
                    }

                    EmuCommand::AutoLoadState {
                        buttons_pressed,
                        dpad_pressed,
                    } => {
                        let path = crate::save_state::auto_save_path(emu.rom_hash);
                        if path.exists() {
                            let resp = match emu.load_state_from_path(&path) {
                                Ok(()) => {
                                    emu.bus
                                        .io
                                        .joypad
                                        .apply_pressed_masks(buttons_pressed, dpad_pressed);
                                    let fb = emu.framebuffer().to_vec();
                                    EmuResponse::LoadStateOk {
                                        path: path.display().to_string(),
                                        framebuffer: fb,
                                    }
                                }
                                Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
                            };
                            if !send_resp(resp) { break 'main; }
                        } else {
                            if !send_resp(EmuResponse::LoadStateFailed("no auto-save".to_string())) { break 'main; }
                        }
                    }

                    EmuCommand::Rewind => {
                        if let Some(rewind_frame) = rewind_buffer.pop() {
                            match emu.load_state_from_bytes(rewind_frame.state_bytes) {
                                Ok(()) => {
                                    let fb = if rewind_frame.framebuffer.is_empty() {
                                        emu.framebuffer().to_vec()
                                    } else {
                                        rewind_frame.framebuffer
                                    };
                                    if !send_resp(EmuResponse::LoadStateOk {
                                        path: "(rewind)".to_string(),
                                        framebuffer: fb,
                                    }) { break 'main; }
                                }
                                Err(err) => {
                                    log::warn!("Rewind restore failed: {}", err);
                                    if !send_resp(EmuResponse::LoadStateFailed(
                                        "rewind restore failed".to_string(),
                                    )) { break 'main; }
                                }
                            }
                        } else {
                            if !send_resp(EmuResponse::LoadStateFailed(
                                "no rewind data".to_string(),
                            )) { break 'main; }
                        }
                    }

                    EmuCommand::Shutdown => {
                        let sram_path = emu.flush_battery_sram().unwrap_or_else(|err| {
                            log::error!("Failed to flush SRAM on shutdown: {}", err);
                            None
                        });
                        let _ = resp_tx.send(EmuResponse::SramFlushed(sram_path));
                        let _ = resp_tx.send(EmuResponse::ShutdownComplete);
                        break 'main;
                    }
                }
                } else {
                    // Self-pacing: uncapped mode, no pending command
                    if matches!(emu.cpu.running, CPUState::Suspended) {
                        std::thread::yield_now();
                        continue;
                    }

                    const UNCAPPED_BATCH: usize = 60;
                    for _ in 0..UNCAPPED_BATCH {
                        emu.step_frame();
                        Self::apply_ram_cheats(&mut emu, &last_cheats);
                        if matches!(emu.cpu.running, CPUState::Suspended) {
                            break;
                        }
                    }

                    let src = emu.framebuffer();
                    let mut frame = uncapped_fb.take().unwrap_or_default();
                    frame.resize(src.len(), 0);
                    frame.copy_from_slice(src);

                    let result = FrameResult {
                        frame,
                        rumble: emu.bus.cartridge.rumble_active(),
                        audio_samples: Vec::new(),
                        ui_data: ui::UiFrameData {
                            debug_info: None,
                            viewer_data: None,
                            disassembly_view: None,
                            rom_info_view: None,
                            memory_page: None,
                            memory_search_results: None,
                            rom_page: None,
                            rom_size: 0,
                            rom_search_results: None,
                        },
                        is_mbc7: emu.is_mbc7_cartridge(),
                        rewind_fill: rewind_buffer.fill_ratio(),
                    };

                    match frame_tx.try_send(result) {
                        Ok(()) => {}
                        Err(TrySendError::Full(result)) => {
                            if let Ok(old) = drain_rx.try_recv() {
                                uncapped_fb = Some(old.frame);
                            }
                            match frame_tx.try_send(result) {
                                Ok(()) => {}
                                Err(TrySendError::Full(result)) => {
                                    uncapped_fb = Some(result.frame);
                                }
                                Err(TrySendError::Disconnected(_)) => break 'main,
                            }
                        }
                        Err(TrySendError::Disconnected(_)) => break 'main,
                    }

                    std::thread::yield_now();
                }
            }
        });

        Self {
            cmd_tx,
            frame_rx,
            resp_rx,
            join: Some(join),
        }
    }

    fn apply_debug_actions(emu: &mut Emulator, actions: &DebugUiActions) {
        if let Some(addr) = actions.add_breakpoint {
            emu.debug.add_breakpoint(addr);
        }
        if let Some((addr, watch_type)) = actions.add_watchpoint {
            emu.debug.add_watchpoint(addr, watch_type);
        }
        for addr in &actions.remove_breakpoints {
            emu.debug.remove_breakpoint(*addr);
        }
        for addr in &actions.toggle_breakpoints {
            emu.debug.toggle_breakpoint(*addr);
        }
        if let Some(mutes) = actions.apu_channel_mutes {
            emu.bus.io.apu.set_channel_mutes(mutes);
        }
        for (addr, value) in &actions.memory_writes {
            emu.bus.write_byte(*addr, *value);
        }
        if let Some((bg, win, sprites)) = actions.layer_toggles {
            emu.bus.io.ppu.debug_enable_bg = bg;
            emu.bus.io.ppu.debug_enable_window = win;
            emu.bus.io.ppu.debug_enable_sprites = sprites;
        }
    }

    fn install_rom_patches(emu: &mut Emulator, cheats: &[crate::cheats::CheatPatch]) {
        use crate::cheats::CheatPatch;
        emu.bus.game_genie_patches.clear();
        for patch in cheats {
            match *patch {
                CheatPatch::RomWrite { .. } | CheatPatch::RomWriteIfEquals { .. } => {
                    emu.bus.game_genie_patches.push(*patch);
                }
                _ => {}
            }
        }
    }

    fn apply_ram_cheats(emu: &mut Emulator, cheats: &[crate::cheats::CheatPatch]) {
        use crate::cheats::CheatPatch;
        for patch in cheats {
            match *patch {
                CheatPatch::RamWrite { address, value } => {
                    let current = emu.bus.read_byte_raw(address);
                    emu.bus.write_byte(address, value.resolve_with_current(current));
                }
                CheatPatch::RamWriteIfEquals { address, value, compare } => {
                    let current = emu.bus.read_byte_raw(address);
                    if compare.matches(current) {
                        emu.bus.write_byte(address, value.resolve_with_current(current));
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn send(&self, cmd: EmuCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub(crate) fn try_recv_frame(&self) -> Option<FrameResult> {
        self.frame_rx.try_recv().ok()
    }

    pub(crate) fn recv(&self) -> Option<EmuResponse> {
        self.resp_rx.recv().ok()
    }


    pub(crate) fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(EmuCommand::Shutdown);
        while self.frame_rx.try_recv().is_ok() {}
        loop {
            match self.resp_rx.recv() {
                Ok(EmuResponse::ShutdownComplete) => break,
                Ok(EmuResponse::SramFlushed(Some(path))) => {
                    log::info!("Saved battery RAM to {}", path);
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for EmuThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}
