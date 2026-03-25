use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{self as chan, Receiver, Sender, TrySendError};

use crate::debug::DebugUiActions;
use zeff_gb_core::emulator::Emulator;
use zeff_gb_core::hardware::types::CPUState;
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
    pub(crate) color_correction: crate::settings::ColorCorrection,
    pub(crate) color_correction_matrix: [f32; 9],
}

pub(crate) struct MemorySearchRequest {
    pub(crate) pattern: Vec<u8>,
    pub(crate) max_results: usize,
}

pub(crate) struct ReusableBuffers {
    pub(crate) framebuffer: Option<Vec<u8>>,
    pub(crate) audio: Option<Vec<f32>>,
    pub(crate) vram: Option<Vec<u8>>,
    pub(crate) oam: Option<Vec<u8>>,
    pub(crate) memory_page: Option<Vec<(u16, u8)>>,
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
    pub(crate) midi_capture_active: bool,
    pub(crate) debug_actions: DebugUiActions,
    pub(crate) snapshot: SnapshotRequest,
    pub(crate) buffers: ReusableBuffers,
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
    pub(crate) apu_snapshot: Option<zeff_gb_core::hardware::apu::ApuChannelSnapshot>,
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
    UpdateCheats(Vec<crate::cheats::CheatPatch>),
    Rewind,
    Shutdown,
}

pub(crate) enum EmuResponse {
    SaveStateOk(String),
    SaveStateFailed(String),
    LoadStateOk { path: String, framebuffer: Vec<u8> },
    LoadStateFailed(String),
    RewindOk { framebuffer: Vec<u8> },
    RewindFailed(String),
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

            let mut rewind_buffer = zeff_gb_core::rewind::RewindBuffer::new(10, 4);
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
                            emu.bus.set_apu_sample_generation_enabled(!on);
                        }

                        EmuCommand::UpdateCheats(cheats) => {
                            last_cheats = cheats;
                            Self::install_rom_patches(&mut emu, &last_cheats);
                        }

                        EmuCommand::StepFrames(input) => {
                            let result = Self::handle_step_frames(
                                &mut emu,
                                input,
                                &last_cheats,
                                uncapped_mode,
                                &mut rewind_buffer,
                                &mut rewind_seconds,
                                &mut cached_rom_info,
                            );

                            if !Self::send_frame(&frame_tx, &drain_rx, result) {
                                break 'main;
                            }
                        }

                        EmuCommand::SaveStateSlot(slot) => {
                            match zeff_gb_core::save_state::slot_path(emu.rom_hash, slot) {
                                Ok(path) => {
                                    if !Self::save_state_async(&emu, path, &resp_tx, &send_resp) {
                                        break 'main;
                                    }
                                }
                                Err(e) => {
                                    if !send_resp(EmuResponse::SaveStateFailed(e.to_string())) {
                                        break 'main;
                                    }
                                }
                            }
                        }

                        EmuCommand::LoadStateSlot {
                            slot,
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            let result = emu.load_state(slot);
                            let path_label = result.as_ref().ok().cloned().unwrap_or_default();
                            let resp = Self::respond_load_state(
                                &mut emu,
                                result.map(|_| ()),
                                path_label,
                                buttons_pressed,
                                dpad_pressed,
                            );
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::SaveStateToPath(path) => {
                            if !Self::save_state_async(&emu, path, &resp_tx, &send_resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::LoadStateFromPath {
                            path,
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            let label = path.display().to_string();
                            let result = emu.load_state_from_path(&path);
                            let resp = Self::respond_load_state(
                                &mut emu,
                                result,
                                label,
                                buttons_pressed,
                                dpad_pressed,
                            );
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::SetSampleRate(rate) => {
                            emu.bus.set_apu_sample_rate(rate);
                        }

                        EmuCommand::CaptureStateBytes => {
                            let resp = match Self::encode_current_state(&emu) {
                                Ok(bytes) => EmuResponse::StateCaptured(bytes),
                                Err(err) => EmuResponse::StateCaptureFailed(err.to_string()),
                            };
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::LoadStateBytes {
                            state_bytes,
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            let result = emu.load_state_from_bytes(state_bytes);
                            let resp = Self::respond_load_state(
                                &mut emu,
                                result,
                                "(replay)".to_string(),
                                buttons_pressed,
                                dpad_pressed,
                            );
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::AutoSaveState => {
                            let path = zeff_gb_core::save_state::auto_save_path(emu.rom_hash);
                            if !Self::save_state_async(&emu, path, &resp_tx, &send_resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::AutoLoadState {
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            let path = zeff_gb_core::save_state::auto_save_path(emu.rom_hash);
                            if path.exists() {
                                let label = path.display().to_string();
                                let result = emu.load_state_from_path(&path);
                                let resp = Self::respond_load_state(
                                    &mut emu,
                                    result,
                                    label,
                                    buttons_pressed,
                                    dpad_pressed,
                                );
                                if !send_resp(resp) {
                                    break 'main;
                                }
                            } else if !send_resp(EmuResponse::LoadStateFailed(
                                "no auto-save".to_string(),
                            )) {
                                break 'main;
                            }
                        }

                        EmuCommand::Rewind => {
                            let resp = Self::handle_rewind(&mut emu, &mut rewind_buffer);
                            if !send_resp(resp) {
                                break 'main;
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
                    Self::run_uncapped_batch(
                        &mut emu,
                        &last_cheats,
                        &mut uncapped_fb,
                        &rewind_buffer,
                        &frame_tx,
                        &drain_rx,
                    );
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

    fn handle_step_frames(
        emu: &mut Emulator,
        input: FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        uncapped_mode: bool,
        rewind_buffer: &mut zeff_gb_core::rewind::RewindBuffer,
        rewind_seconds: &mut usize,
        cached_rom_info: &mut Option<crate::debug::RomInfoViewData>,
    ) -> FrameResult {
        Self::apply_debug_actions(emu, &input.debug_actions);

        if emu
            .bus
            .apply_joypad_pressed_masks(input.buttons_pressed, input.dpad_pressed)
        {
            emu.bus.if_reg |= 0x10;
        }
        emu.set_mbc7_host_tilt(input.host_tilt.0, input.host_tilt.1);
        emu.bus
            .set_apu_debug_capture_enabled(input.apu_capture_enabled);
        if !uncapped_mode {
            emu.bus.set_apu_sample_generation_enabled(!input.skip_audio);
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
                Self::apply_ram_cheats(emu, cheats);
                if matches!(emu.cpu.running, CPUState::Suspended) {
                    break;
                }
            }
        }

        if input.rewind_seconds != *rewind_seconds {
            *rewind_seconds = input.rewind_seconds;
            *rewind_buffer = zeff_gb_core::rewind::RewindBuffer::new(*rewind_seconds, 4);
        }

        Self::capture_rewind_snapshot(emu, rewind_buffer, input.rewind_enabled);

        let ui_data = {
            if cached_rom_info.is_none() && input.snapshot.show_rom_info {
                *cached_rom_info = Some(ui::compute_static_rom_info(emu));
            }
            ui::collect_emu_snapshot(
                emu,
                &input.snapshot,
                cached_rom_info,
                input.buffers.vram,
                input.buffers.oam,
                input.buffers.memory_page,
            )
        };

        Self::build_frame_result(
            emu,
            input.buffers.framebuffer,
            input.buffers.audio,
            ui_data,
            input.midi_capture_active,
            rewind_buffer.fill_ratio(),
        )
    }

    fn respond_load_state(
        emu: &mut Emulator,
        result: anyhow::Result<()>,
        path_label: String,
        buttons_pressed: u8,
        dpad_pressed: u8,
    ) -> EmuResponse {
        match result {
            Ok(()) => {
                emu.bus
                    .apply_joypad_pressed_masks(buttons_pressed, dpad_pressed);
                let fb = emu.framebuffer().to_vec();
                EmuResponse::LoadStateOk {
                    path: path_label,
                    framebuffer: fb,
                }
            }
            Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
        }
    }

    fn save_state_async(
        emu: &Emulator,
        path: PathBuf,
        resp_tx: &Sender<EmuResponse>,
        send_resp: &impl Fn(EmuResponse) -> bool,
    ) -> bool {
        match Self::encode_current_state(emu) {
            Ok(bytes) => {
                let tx = resp_tx.clone();
                std::thread::spawn(move || {
                    let resp = match zeff_gb_core::save_state::write_state_bytes_to_file(&path, &bytes) {
                        Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                        Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                    };
                    let _ = tx.send(resp);
                });
                true
            }
            Err(err) => send_resp(EmuResponse::SaveStateFailed(err.to_string())),
        }
    }

    fn handle_rewind(
        emu: &mut Emulator,
        rewind_buffer: &mut zeff_gb_core::rewind::RewindBuffer,
    ) -> EmuResponse {
        if let Some(rewind_frame) = rewind_buffer.pop() {
            match emu.load_state_from_bytes(rewind_frame.state_bytes) {
                Ok(()) => {
                    let fb = if rewind_frame.framebuffer.is_empty() {
                        emu.framebuffer().to_vec()
                    } else {
                        rewind_frame.framebuffer
                    };
                    EmuResponse::RewindOk { framebuffer: fb }
                }
                Err(err) => {
                    log::warn!("Rewind restore failed: {}", err);
                    EmuResponse::RewindFailed("rewind restore failed".to_string())
                }
            }
        } else {
            EmuResponse::RewindFailed("no rewind data".to_string())
        }
    }

    fn build_frame_result(
        emu: &mut Emulator,
        reusable_fb: Option<Vec<u8>>,
        reusable_audio: Option<Vec<f32>>,
        ui_data: ui::UiFrameData,
        midi_capture_active: bool,
        rewind_fill: f32,
    ) -> FrameResult {
        let src = emu.framebuffer();
        let mut frame = reusable_fb.unwrap_or_default();
        frame.resize(src.len(), 0);
        frame.copy_from_slice(src);

        let rumble = emu.bus.cartridge.rumble_active();
        let audio_samples = if let Some(mut buf) = reusable_audio {
            emu.bus.apu_drain_samples_into(&mut buf);
            buf
        } else {
            emu.bus.apu_drain_samples()
        };
        let is_mbc7 = emu.is_mbc7_cartridge();

        let apu_snapshot = if midi_capture_active {
            Some(emu.bus.apu_channel_snapshot())
        } else {
            None
        };

        FrameResult {
            frame,
            rumble,
            audio_samples,
            ui_data,
            is_mbc7,
            rewind_fill,
            apu_snapshot,
        }
    }

    fn send_frame(
        frame_tx: &Sender<FrameResult>,
        drain_rx: &Receiver<FrameResult>,
        result: FrameResult,
    ) -> bool {
        match frame_tx.try_send(result) {
            Ok(()) => true,
            Err(TrySendError::Full(result)) => {
                let _ = drain_rx.try_recv();
                !frame_tx.try_send(result).is_err()
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn run_uncapped_batch(
        emu: &mut Emulator,
        cheats: &[crate::cheats::CheatPatch],
        uncapped_fb: &mut Option<Vec<u8>>,
        rewind_buffer: &zeff_gb_core::rewind::RewindBuffer,
        frame_tx: &Sender<FrameResult>,
        drain_rx: &Receiver<FrameResult>,
    ) {
        if matches!(emu.cpu.running, CPUState::Suspended) {
            std::thread::yield_now();
            return;
        }

        const UNCAPPED_BATCH: usize = 60;
        for _ in 0..UNCAPPED_BATCH {
            emu.step_frame();
            Self::apply_ram_cheats(emu, cheats);
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
            apu_snapshot: None,
        };

        match frame_tx.try_send(result) {
            Ok(()) => {}
            Err(TrySendError::Full(result)) => {
                if let Ok(old) = drain_rx.try_recv() {
                    *uncapped_fb = Some(old.frame);
                }
                match frame_tx.try_send(result) {
                    Ok(()) => {}
                    Err(TrySendError::Full(result)) => {
                        *uncapped_fb = Some(result.frame);
                    }
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
            Err(TrySendError::Disconnected(_)) => return,
        }

        std::thread::yield_now();
    }

    fn encode_current_state(emu: &Emulator) -> anyhow::Result<Vec<u8>> {
        emu.encode_state_bytes()
    }

    fn capture_rewind_snapshot(
        emu: &Emulator,
        rewind_buffer: &mut zeff_gb_core::rewind::RewindBuffer,
        enabled: bool,
    ) {
        if enabled && rewind_buffer.tick() {
            if let Ok(bytes) = Self::encode_current_state(emu) {
                rewind_buffer.push(&bytes, emu.framebuffer());
            }
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
            emu.bus.set_apu_channel_mutes(mutes);
        }
        for (addr, value) in &actions.memory_writes {
            emu.bus.write_byte(*addr, *value);
        }
        if let Some((bg, win, sprites)) = actions.layer_toggles {
            emu.bus.set_ppu_debug_flags(bg, win, sprites);
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
                    emu.bus
                        .write_byte(address, value.resolve_with_current(current));
                }
                CheatPatch::RamWriteIfEquals {
                    address,
                    value,
                    compare,
                } => {
                    let current = emu.bus.read_byte_raw(address);
                    if compare.matches(current) {
                        emu.bus
                            .write_byte(address, value.resolve_with_current(current));
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

    pub(crate) fn try_recv_response(&self) -> Option<EmuResponse> {
        self.resp_rx.try_recv().ok()
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
