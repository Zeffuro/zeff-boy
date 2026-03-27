use crossbeam_channel::{Sender, TrySendError};

use crate::emu_backend::EmuBackend;
use crate::ui;

use super::{EmuThread, FrameInput, FrameResult};

impl EmuThread {
    pub(crate) fn handle_step_frames(
        backend: &mut EmuBackend,
        input: FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        uncapped_mode: bool,
        rewind_buffer: &mut zeff_gb_core::rewind::RewindBuffer,
        rewind_seconds: &mut usize,
    ) -> FrameResult {
        if let Some(gb) = backend.gb_mut() {
            Self::apply_debug_actions(&mut gb.emu, &input.debug_actions);
        }

        if let Some(nes) = backend.nes_mut() {
            Self::apply_nes_debug_actions(&mut nes.emu, &input.debug_actions);
        }

        backend.set_input(input.buttons_pressed, input.dpad_pressed);
        backend.set_input_p2(input.buttons_pressed_p2, input.dpad_pressed_p2);

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        if let Some(gb) = backend.gb_mut() {
            gb.emu.set_mbc7_host_tilt(input.host_tilt.0, input.host_tilt.1);
            gb.emu.set_apu_debug_capture_enabled(input.apu_capture_enabled);
            if !uncapped_mode {
                gb.emu.set_apu_sample_generation_enabled(!input.skip_audio);
            }
            gb.emu.set_opcode_log_enabled(input.snapshot.want_debug_info);

            if gb.emu.is_cpu_suspended() {
                if input.debug_continue {
                    gb.emu.debug_continue();
                } else if input.debug_step {
                    gb.emu.debug_step();
                }
            }
        }

        if let Some(nes) = backend.nes_mut()
            && nes.emu.is_cpu_suspended()
        {
            if input.debug_continue {
                nes.emu.debug_continue();
            } else if input.debug_step {
                nes.emu.debug_step();
            }
        }

        if input.frames > 0 && backend.is_running() {
            for _ in 0..input.frames {
                backend.step_frame();
                if let Some(gb) = backend.gb_mut() {
                    Self::apply_ram_cheats(&mut gb.emu, cheats);
                }
                if let Some(nes) = backend.nes_mut() {
                    Self::apply_nes_ram_cheats(&mut nes.emu, cheats);
                }
                if backend.is_suspended() {
                    break;
                }
            }
        }

        if input.rewind_seconds != *rewind_seconds {
            *rewind_seconds = input.rewind_seconds;
            *rewind_buffer = zeff_gb_core::rewind::RewindBuffer::new(*rewind_seconds, 4);
        }

        Self::capture_rewind_snapshot(backend, rewind_buffer, input.rewind_enabled);

        let ui_data = match backend {
            EmuBackend::Gb(gb) => {
                ui::collect_emu_snapshot(
                    &gb.emu,
                    &input.snapshot,
                    input.buffers.vram,
                    input.buffers.oam,
                    input.buffers.memory_page,
                )
            }
            EmuBackend::Nes(nes) => {
                let emu = &mut nes.emu;
                let mut data = ui::empty_frame_data();

                emu.set_apu_debug_collection_enabled(input.snapshot.show_apu_viewer);
                emu.set_opcode_log_enabled(input.snapshot.want_debug_info);

                if input.snapshot.want_perf_info {
                    data.perf_info = Some(crate::debug::PerfInfo {
                        fps: 0.0,
                        speed_mode_label: "1×".to_string(),
                        frames_in_flight: 0,
                        cycles: emu.cpu_cycles(),
                        platform_name: "NES",
                        hardware_label: emu.cartridge_header().mapper_label(),
                        hardware_pref_label: format!("{:?}", emu.cartridge_header().timing),
                    });
                }

                if input.snapshot.want_debug_info {
                    data.cpu_debug = Some(ui::nes_cpu_snapshot(emu));
                }

                if input.snapshot.show_apu_viewer {
                    data.apu_debug = ui::nes_apu_snapshot(emu, true);
                }

                if input.snapshot.show_disassembler {
                    let pc_changed = input.snapshot.last_disasm_pc != Some(emu.cpu_pc());
                    if pc_changed {
                        data.disassembly_view = Some(ui::nes_disassembly_view(emu));
                    }
                }

                if input.snapshot.show_rom_info {
                    data.rom_debug = Some(ui::nes_rom_info(emu));
                }

                if input.snapshot.show_memory_viewer {
                    let start = input.snapshot.memory_view_start;
                    let mut page = Vec::with_capacity(256);
                    for i in 0..256u16 {
                        let addr = start.wrapping_add(i);
                        page.push((addr, emu.cpu_peek(addr)));
                    }
                    data.memory_page = Some(page);
                }

                if input.snapshot.show_rom_viewer {
                    let rom_header = emu.cartridge_header();
                    let prg_size = rom_header.prg_rom_size;
                    data.rom_size = prg_size as u32;
                    let start = input.snapshot.rom_view_start as usize;
                    let mut page = Vec::with_capacity(256);
                    for i in 0..256usize {
                        let offset = start + i;
                        if offset < prg_size {
                            let addr = 0x8000u16.wrapping_add(offset as u16);
                            page.push((offset as u32, emu.cpu_peek(addr)));
                        }
                    }
                    data.rom_page = Some(page);
                }

                data
            }
        };

        Self::build_frame_result(
            backend,
            input.buffers.framebuffer,
            input.buffers.audio,
            ui_data,
            input.midi_capture_active,
            rewind_buffer.fill_ratio(),
        )
    }

    pub(crate) fn build_frame_result(
        backend: &mut EmuBackend,
        reusable_fb: Option<Vec<u8>>,
        reusable_audio: Option<Vec<f32>>,
        ui_data: ui::UiFrameData,
        midi_capture_active: bool,
        rewind_fill: f32,
    ) -> FrameResult {
        let src = backend.framebuffer();
        let mut frame = reusable_fb.unwrap_or_default();
        frame.resize(src.len(), 0);
        frame.copy_from_slice(src);

        let rumble = backend.rumble_active();
        let audio_samples = if let Some(mut buf) = reusable_audio {
            backend.drain_audio_samples_into(&mut buf);
            buf
        } else {
            backend.drain_audio_samples()
        };
        let is_mbc7 = backend.is_mbc7();

        let apu_snapshot = if midi_capture_active {
            backend.apu_channel_snapshot()
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

    pub(crate) fn send_frame(
        frame_tx: &Sender<FrameResult>,
        drain_rx: &crossbeam_channel::Receiver<FrameResult>,
        result: FrameResult,
    ) -> bool {
        match frame_tx.try_send(result) {
            Ok(()) => true,
            Err(TrySendError::Full(result)) => {
                let _ = drain_rx.try_recv();
                frame_tx.try_send(result).is_ok()
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub(crate) fn run_uncapped_batch(
        backend: &mut EmuBackend,
        cheats: &[crate::cheats::CheatPatch],
        uncapped_fb: &mut Option<Vec<u8>>,
        rewind_buffer: &zeff_gb_core::rewind::RewindBuffer,
        frame_tx: &Sender<FrameResult>,
        drain_rx: &crossbeam_channel::Receiver<FrameResult>,
    ) {
        if backend.is_suspended() {
            std::thread::yield_now();
            return;
        }

        const UNCAPPED_BATCH: usize = 60;
        for _ in 0..UNCAPPED_BATCH {
            backend.step_frame();
            if let Some(gb) = backend.gb_mut() {
                Self::apply_ram_cheats(&mut gb.emu, cheats);
            }
            if let Some(nes) = backend.nes_mut() {
                Self::apply_nes_ram_cheats(&mut nes.emu, cheats);
            }
            if backend.is_suspended() {
                break;
            }
        }

        let src = backend.framebuffer();
        let mut frame = uncapped_fb.take().unwrap_or_default();
        frame.resize(src.len(), 0);
        frame.copy_from_slice(src);

        let result = FrameResult {
            frame,
            rumble: backend.rumble_active(),
            audio_samples: Vec::new(),
            ui_data: ui::empty_frame_data(),
            is_mbc7: backend.is_mbc7(),
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
}

