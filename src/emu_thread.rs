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
}

pub(crate) struct FrameResult {
    pub(crate) frame: Vec<u8>,
    pub(crate) rumble: bool,
    pub(crate) audio_samples: Vec<f32>,
    pub(crate) ui_data: ui::UiFrameData,
    pub(crate) is_mbc7: bool,
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
    SetSampleRate(u32),
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
            while let Ok(command) = cmd_rx.recv() {
                match command {
                    EmuCommand::StepFrames(input) => {
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
                        emu.bus.io.apu.sample_generation_enabled = !input.skip_audio;

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
                                if matches!(emu.cpu.running, CPUState::Suspended) {
                                    break;
                                }
                            }
                        }

                        let ui_data = ui::collect_emu_snapshot(&emu, &input.snapshot);

                        let src = emu.framebuffer();
                        let mut frame = input.reusable_framebuffer.unwrap_or_default();
                        frame.resize(src.len(), 0);
                        frame.copy_from_slice(src);

                        let rumble = emu.bus.cartridge.rumble_active();
                        let audio_samples = emu.bus.io.apu.drain_samples();
                        let is_mbc7 = emu.is_mbc7_cartridge();

                        let result = FrameResult {
                            frame,
                            rumble,
                            audio_samples,
                            ui_data,
                            is_mbc7,
                        };
                        
                        match frame_tx.try_send(result) {
                            Ok(()) => {}
                            Err(TrySendError::Full(result)) => {
                                let _ = drain_rx.try_recv(); // drop stale frame
                                if frame_tx.try_send(result).is_err() {
                                    break; // disconnected
                                }
                            }
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    }

                    EmuCommand::SaveStateSlot(slot) => {
                        let resp = match emu.save_state(slot) {
                            Ok(path) => EmuResponse::SaveStateOk(path),
                            Err(err) => EmuResponse::SaveStateFailed(err.to_string()),
                        };
                        if resp_tx.send(resp).is_err() {
                            break;
                        }
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
                        if resp_tx.send(resp).is_err() {
                            break;
                        }
                    }

                    EmuCommand::SaveStateToPath(path) => {
                        let resp = match emu.save_state_to_path(&path) {
                            Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                            Err(err) => EmuResponse::SaveStateFailed(err.to_string()),
                        };
                        if resp_tx.send(resp).is_err() {
                            break;
                        }
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
                        if resp_tx.send(resp).is_err() {
                            break;
                        }
                    }

                    EmuCommand::SetSampleRate(rate) => {
                        emu.bus.io.apu.set_sample_rate(rate);
                    }

                    EmuCommand::Shutdown => {
                        let sram_path = match emu.flush_battery_sram() {
                            Ok(path) => path,
                            Err(err) => {
                                log::error!("Failed to flush SRAM on shutdown: {}", err);
                                None
                            }
                        };
                        let _ = resp_tx.send(EmuResponse::SramFlushed(sram_path));
                        let _ = resp_tx.send(EmuResponse::ShutdownComplete);
                        break;
                    }
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
        #[cfg(debug_assertions)]
        for (addr, value) in &actions.memory_writes {
            emu.bus.write_byte(*addr, *value);
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
