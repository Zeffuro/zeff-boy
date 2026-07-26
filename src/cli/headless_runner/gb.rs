use super::*;

pub(super) fn run_gb_headless(
    path: &Path,
    rom_data: &[u8],
    mode_preference: HardwareModePreference,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let mut emulator = GbEmulator::from_rom_data(rom_data, mode_preference)?;
    if !opts.no_sram
        && let Some(sram_path) = crate::emu_backend::gb::try_load_battery_sram(&mut emulator, path)
            .unwrap_or_else(|e| {
                log::warn!("Failed to load battery save: {e}");
                None
            })
    {
        log::info!("Loaded battery save from {}", sram_path);
    }
    if opts.no_apu {
        emulator.set_apu_enabled(false);
        emulator.set_apu_sample_generation_enabled(false);
        log::info!("APU disabled for profiling");
    }
    if let Some(bytes) = read_headless_state_if_requested(opts)? {
        emulator.load_state_from_bytes(bytes)?;
        log::info!(
            "Loaded save state from {}",
            opts.load_state_path.as_ref().unwrap().display()
        );
    }
    ensure_no_reset_events("gb", opts)?;
    let flush_battery = |emulator: &GbEmulator| {
        if opts.no_sram {
            return;
        }
        match crate::save_paths::flush_battery_sram(path, emulator.dump_battery_sram()) {
            Ok(Some(save_path)) => log::info!("Saved battery RAM to {}", save_path),
            Ok(None) => {}
            Err(err) => log::error!("Failed to save battery RAM: {}", err),
        }
    };
    let check_breakpoint = |emulator: &GbEmulator| -> Option<anyhow::Result<()>> {
        if !emulator.is_cpu_suspended() {
            return None;
        }
        println!(
            "{}",
            format_headless_breakpoint(
                emulator.cpu_pc(),
                emulator.cpu_cycles(),
                emulator.cpu_a(),
                emulator.cpu_f(),
                emulator.cpu_sp(),
            )
        );
        flush_battery(emulator);
        Some(Ok(()))
    };
    if let Some(addr) = opts.break_at {
        emulator.add_breakpoint(addr);
    }
    let mut traced = 0u64;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let mut screenshot_written = false;
    let mut current_input = InputMasks::default();
    let start = Instant::now();
    let mut frames_run = 0u64;
    write_screenshot_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        emulator.framebuffer_dimensions(),
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        emulator.framebuffer_dimensions(),
    )?;

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        emulator.set_input(current_input.buttons, current_input.dpad);

        if opts.trace_opcodes {
            let target = emulator
                .cpu_cycles()
                .wrapping_add(GbEmulator::cycles_per_frame(emulator.hardware_mode()));
            while emulator.cpu_cycles() < target {
                let (pc, op, cb_prefix, step_cycles) = emulator.step_instruction();
                if let Some(result) = check_breakpoint(&emulator) {
                    return result;
                }
                let if_reg = emulator.if_reg();
                let ie = emulator.ie_reg();
                let ime = emulator.cpu_ime();
                if (opts.trace_opcode_limit == 0 || traced < opts.trace_opcode_limit)
                    && should_trace_op(
                        opts,
                        &crate::cli::trace_filters::CpuTraceState {
                            pc: u64::from(pc),
                            op,
                            total_t: emulator.cpu_cycles(),
                            ime: &ime,
                            if_reg,
                            ie,
                        },
                    )
                {
                    let pending = (if_reg & ie) & 0x1F;
                    let op1 = emulator.peek_byte(pc.wrapping_add(1));
                    let op2 = emulator.peek_byte(pc.wrapping_add(2));
                    let mut op_extra = String::new();
                    if !cb_prefix {
                        match op {
                            0xFA => {
                                let addr = u16::from_le_bytes([op1, op2]);
                                let value = emulator.peek_byte(addr);
                                op_extra = format!(" fa_addr={:04X} fa_val={:02X}", addr, value);
                            }
                            0xF0 => {
                                let addr = 0xFF00u16 | u16::from(op1);
                                let value = emulator.peek_byte(addr);
                                op_extra = format!(" f0_addr={:04X} f0_val={:02X}", addr, value);
                            }
                            0xE0 => {
                                let addr = 0xFF00u16 | u16::from(op1);
                                op_extra = format!(" e0_addr={:04X}", addr);
                            }
                            0xC4 => {
                                let target = u16::from_le_bytes([op1, op2]);
                                let taken = if step_cycles >= 24 { 1 } else { 0 };
                                op_extra = format!(" c4_target={:04X} c4_taken={}", target, taken);
                            }
                            _ => {}
                        }
                    }
                    let f = emulator.cpu_f();
                    let zf = (f >> 7) & 1;
                    let nf = (f >> 6) & 1;
                    let hf = (f >> 5) & 1;
                    let cf = (f >> 4) & 1;

                    let ctx = TraceContext {
                        pc,
                        op,
                        cb_prefix,
                        step_cycles,
                        total_t: emulator.cpu_cycles(),
                        ime: ime_short(&ime),
                        if_reg,
                        ie,
                        pending,
                        div: emulator.timer_div(),
                        tima: emulator.timer_tima(),
                        tac: emulator.timer_tac(),
                        a: emulator.cpu_a(),
                        f,
                        zf,
                        nf,
                        hf,
                        cf,
                        mode: mode_short(emulator.hardware_mode()),
                        op_extra: &op_extra,
                    };

                    let op_line = format_op_line(traced, &ctx);
                    println!("{}", op_line);

                    traced = traced.wrapping_add(1);
                    let tail_line = format_op_tail_line(&ctx);
                    if tail.len() == 64 {
                        tail.pop_front();
                    }
                    tail.push_back(tail_line);
                }
            }
        } else {
            emulator.step_frame();
            if let Some(result) = check_breakpoint(&emulator) {
                return result;
            }
        }

        frames_run = frame_number;
        write_screenshot_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            emulator.framebuffer_dimensions(),
            &mut screenshot_written,
        )?;
        write_screenshot_sequence_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            emulator.framebuffer_dimensions(),
        )?;
        observe_stuck(
            &mut stuck,
            "gb",
            4,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
            None,
            false,
            &mut stuck_active,
        );
    }

    if opts.trace_opcodes {
        println!("[op-tail] ---- last {} ops ----", tail.len());
        for line in tail {
            println!("{}", line);
        }
    }

    let serial_bytes = emulator.serial_output_bytes();
    let serial_text = String::from_utf8_lossy(serial_bytes);
    println!(
        "{}",
        format_headless_summary(
            frames_run,
            emulator.cpu_cycles(),
            emulator.cpu_pc(),
            serial_bytes.len()
        )
    );
    print_perf("gb", frames_run, start);
    if !serial_text.is_empty() {
        println!("{}", format_headless_serial(&serial_text));
    }

    write_final_screenshot_if_needed(
        opts,
        frames_run,
        emulator.framebuffer(),
        emulator.framebuffer_dimensions(),
        &mut screenshot_written,
    )?;
    emit_debug_state(
        opts,
        gb_debug_state(
            &emulator,
            frames_run,
            opts,
            current_input,
            stuck.as_ref().and_then(StuckTracker::current_report),
            screenshot_path_if_written(opts, screenshot_written),
        ),
    )?;

    if let Some(expected) = &opts.expect_serial
        && !serial_text.contains(expected)
    {
        flush_battery(&emulator);
        anyhow::bail!(
            "expected serial output containing {:?}, got {:?}",
            expected,
            serial_text
        );
    }

    flush_battery(&emulator);
    fail_on_stuck_if_needed("gb", stuck.as_ref(), opts)?;

    Ok(())
}
