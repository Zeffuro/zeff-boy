use super::*;

pub(super) fn run_nes_headless(
    path: &Path,
    rom_data: &[u8],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options("nes", opts)?;

    let mut emulator = NesEmulator::new(rom_data, zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE)?;
    if !opts.no_sram
        && let Some(sram_path) = crate::emu_backend::nes::try_load_battery_sram(&mut emulator, path)
            .unwrap_or_else(|e| {
                log::warn!("Failed to load battery save: {e}");
                None
            })
    {
        log::info!("Loaded battery save from {}", sram_path);
    }
    if opts.no_apu {
        emulator.set_apu_sample_generation_enabled(false);
        log::info!("APU sample generation disabled for profiling");
    }
    if let Some(bytes) = read_headless_state_if_requested(opts)? {
        emulator.load_state_from_bytes(bytes)?;
        log::info!(
            "Loaded save state from {}",
            opts.load_state_path.as_ref().unwrap().display()
        );
    }

    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let mut screenshot_written = false;
    let mut current_input = InputMasks::default();
    let start = Instant::now();
    let mut frames_run = 0u64;
    let mut traced = 0u64;
    let mut bus_traced = 0u64;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let bus_trace_active = !opts.trace_bus_filters.is_empty();
    write_screenshot_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        (256, 240),
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(opts, 0, emulator.framebuffer(), (256, 240))?;

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        if current_input.reset {
            emulator.reset();
        }
        emulator.set_input_p1(map_host_to_nes_byte(
            current_input.buttons,
            current_input.dpad,
        ));

        if opts.trace_opcodes || bus_trace_active {
            emulator.clear_frame_ready();
            let start_cycles = emulator.cpu_cycles();
            let max_cycles = zeff_nes_core::emulator::CPU_CYCLES_PER_FRAME * 2;
            while !emulator.ppu_frame_ready()
                && emulator.cpu_cycles().wrapping_sub(start_cycles) < max_cycles
                && !emulator.is_cpu_suspended()
            {
                let bus_trace_collecting = bus_trace_active
                    && (opts.trace_bus_limit == 0 || bus_traced < opts.trace_bus_limit);
                let (pc, op, step_cycles, bus_events) = if bus_trace_collecting {
                    emulator.step_instruction_with_bus_trace()
                } else {
                    let (pc, op, step_cycles) = emulator.step_instruction();
                    (pc, op, step_cycles, Vec::new())
                };
                if opts.trace_opcodes
                    && (opts.trace_opcode_limit == 0 || traced < opts.trace_opcode_limit)
                    && should_trace_nes_op(opts, pc, op, emulator.cpu_cycles())
                {
                    let op1 = emulator.cpu_peek(pc.wrapping_add(1));
                    let op2 = emulator.cpu_peek(pc.wrapping_add(2));
                    let line = format_nes_op_line(
                        traced,
                        &emulator,
                        pc,
                        op,
                        op1,
                        op2,
                        step_cycles,
                        nes_op_extra(pc, op, op1, op2).as_deref().unwrap_or(""),
                    );
                    println!("{}", line);
                    traced = traced.wrapping_add(1);
                    if tail.len() == 64 {
                        tail.pop_front();
                    }
                    tail.push_back(line.replacen("[nes-op]", "[nes-op-tail]", 1));
                }
                if bus_trace_collecting && emulator.cpu_cycles() >= opts.trace_start_t {
                    for event in bus_events {
                        if opts.trace_bus_limit != 0 && bus_traced >= opts.trace_bus_limit {
                            break;
                        }
                        if should_trace_nes_bus_event(opts, event) {
                            println!(
                                "{}",
                                format_nes_bus_trace_line(bus_traced, &emulator, pc, op, event)
                            );
                            bus_traced = bus_traced.wrapping_add(1);
                        }
                    }
                }
            }
        } else {
            emulator.step_frame();
        }
        frames_run = frame_number;

        write_screenshot_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            (256, 240),
            &mut screenshot_written,
        )?;
        write_screenshot_sequence_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            (256, 240),
        )?;

        observe_stuck(
            &mut stuck,
            "nes",
            4,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
            None,
            false,
            &mut stuck_active,
        );

        if emulator.is_cpu_suspended() {
            println!(
                "[headless] system=nes suspended frame={} cycles={} pc={}",
                frames_run,
                emulator.cpu_cycles(),
                format_pc(u64::from(emulator.cpu_pc()), 4)
            );
            break;
        }
    }

    if opts.trace_opcodes {
        println!("[nes-op-tail] ---- last {} ops ----", tail.len());
        for line in tail {
            println!("{}", line);
        }
    }

    println!(
        "[headless] system=nes frames={} cycles={} pc={} mapper={} battery={}",
        frames_run,
        emulator.cpu_cycles(),
        format_pc(u64::from(emulator.cpu_pc()), 4),
        emulator.cartridge_effective_mapper_label(),
        emulator.has_battery()
    );
    print_perf("nes", frames_run, start);
    write_final_screenshot_if_needed(
        opts,
        frames_run,
        emulator.framebuffer(),
        (256, 240),
        &mut screenshot_written,
    )?;
    emit_debug_state(
        opts,
        nes_debug_state(
            &mut emulator,
            frames_run,
            opts,
            current_input,
            stuck.as_ref().and_then(StuckTracker::current_report),
            screenshot_path_if_written(opts, screenshot_written),
        ),
    )?;
    if !opts.no_sram {
        flush_battery(path, emulator.dump_battery_sram());
    }
    fail_on_stuck_if_needed("nes", stuck.as_ref(), opts)?;

    Ok(())
}
