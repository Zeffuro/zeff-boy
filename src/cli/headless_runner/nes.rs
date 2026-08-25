use super::*;

pub(super) fn run_nes_headless(
    path: &Path,
    rom_data: &[u8],
    firmware_search_dirs: &[std::path::PathBuf],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options("nes", opts)?;

    let mut emulator = if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fds"))
    {
        let bios =
            crate::emu_backend::firmware::resolve_fds_bios(None, firmware_search_dirs, Some(path))?;
        NesEmulator::new_fds(rom_data, bios, zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE)?
    } else {
        NesEmulator::new(rom_data, zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE)?
    };
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
    let mut current_input_p2 = InputMasks::default();
    let start = Instant::now();
    let mut frames_run = 0u64;
    let mut traced = 0u64;
    let mut bus_traced = 0u64;
    let mut test_pass_seen = false;
    let mut test_status_seen = false;
    let mut last_test_status: Option<(u8, String)> = None;
    let mut reset_at_frame: Option<u64> = None;
    let mut reset_requests = 0u8;
    let mut reset_request_active = false;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let bus_trace_active = !opts.trace_bus_filters.is_empty();
    let mut test_output_shadow = (opts.expect_test_pass
        && emulator
            .cartridge_effective_mapper_label()
            .starts_with("CNROM"))
    .then(|| Box::new([0; 0x2000]));
    write_screenshot_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        (256, 240),
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(opts, 0, emulator.framebuffer(), (256, 240))?;

    'frames: for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        current_input_p2 = input_p2_for_frame(opts, frame_number);
        if current_input.reset {
            emulator.reset();
        }
        if reset_at_frame.is_some_and(|target| frame_number >= target) {
            emulator.reset();
            reset_at_frame = None;
            println!("[headless] nes-test scripted-reset frame={frame_number}");
        }
        emulator.set_input(current_input.buttons, current_input.dpad);
        emulator.set_input_p2(current_input_p2.buttons, current_input_p2.dpad);
        emulator.set_zapper_state(
            current_input.zapper_enabled,
            current_input.zapper_trigger,
            current_input.zapper_hit,
            current_input.zapper_screen_pos,
        );

        if opts.trace_opcodes || bus_trace_active || test_output_shadow.is_some() {
            emulator.clear_frame_ready();
            let start_cycles = emulator.cpu_cycles();
            let max_cycles = zeff_nes_core::emulator::CPU_CYCLES_PER_FRAME * 2;
            while !emulator.ppu_frame_ready()
                && emulator.cpu_cycles().wrapping_sub(start_cycles) < max_cycles
                && !emulator.is_cpu_suspended()
            {
                let bus_trace_collecting = bus_trace_active
                    && (opts.trace_bus_limit == 0 || bus_traced < opts.trace_bus_limit);
                let (pc, op, step_cycles, bus_events) =
                    if bus_trace_collecting || test_output_shadow.is_some() {
                        emulator.step_instruction_with_bus_trace()
                    } else {
                        let (pc, op, step_cycles) = emulator.step_instruction();
                        (pc, op, step_cycles, Vec::new())
                    };
                if let Some(shadow) = test_output_shadow.as_mut() {
                    for event in &bus_events {
                        if let zeff_nes_core::hardware::bus::DebugTraceEvent::Write {
                            addr,
                            written_value,
                            ..
                        } = event
                            && (0x6000..=0x7FFF).contains(addr)
                        {
                            shadow[*addr as usize - 0x6000] = *written_value as u8;
                        }
                    }
                }
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
            None,
            false,
            &mut stuck_active,
        );

        if opts.expect_test_pass
            && let Some((status, text)) =
                read_nes_memory_test_status(&emulator, test_output_shadow.as_deref())
        {
            test_status_seen = true;
            last_test_status = Some((status, text.clone()));
            match status {
                0x00 => {
                    println!("[headless] nes-test result=pass status=00 text={text:?}");
                    test_pass_seen = true;
                    break 'frames;
                }
                0x01..=0x7F => {
                    anyhow::bail!("NES test failed with status={status:02X} text={text:?}");
                }
                0x81 => {
                    if !reset_request_active && reset_at_frame.is_none() {
                        reset_request_active = true;
                        reset_requests = reset_requests.saturating_add(1);
                        if reset_requests > 8 {
                            anyhow::bail!(
                                "NES test requested more than 8 scripted resets; last text={text:?}"
                            );
                        }
                        reset_at_frame = Some(frame_number + 7);
                        println!(
                            "[headless] nes-test reset-request status=81 frame={frame_number} reset_at_frame={} text={text:?}",
                            frame_number + 7
                        );
                    }
                }
                _ => {
                    reset_request_active = false;
                }
            }
        }

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
        nes_debug_state(NesDebugStateRequest {
            emulator: &mut emulator,
            frames_run,
            opts,
            input: current_input,
            input_p2: current_input_p2,
            stuck: stuck.as_ref().and_then(StuckTracker::current_report),
            screenshot: screenshot_path_if_written(opts, screenshot_written),
        }),
    )?;
    if opts.expect_test_pass && !test_pass_seen {
        if test_status_seen {
            if let Some((status, text)) = last_test_status {
                anyhow::bail!(
                    "expected NES memory-status test pass before max frame limit, last status={status:02X} text={text:?}"
                );
            }
        } else {
            anyhow::bail!(
                "expected NES memory-status test pass before max frame limit, but no $6000 signature was observed"
            );
        }
    }
    if !opts.no_sram {
        flush_battery(path, emulator.dump_persistent_data());
    }
    fail_on_stuck_if_needed("nes", stuck.as_ref(), opts)?;

    Ok(())
}

fn read_nes_memory_test_status(
    emulator: &NesEmulator,
    output_shadow: Option<&[u8; 0x2000]>,
) -> Option<(u8, String)> {
    let read = |addr: u16| {
        output_shadow.map_or_else(
            || emulator.cpu_peek(addr),
            |shadow| shadow[addr as usize - 0x6000],
        )
    };
    let signature = [read(0x6001), read(0x6002), read(0x6003)];
    if signature != [0xDE, 0xB0, 0x61] {
        return None;
    }

    let status = read(0x6000);
    let mut text = Vec::new();
    for addr in 0x6004..=0x7FFF {
        let byte = read(addr);
        if byte == 0 {
            break;
        }
        text.push(byte);
        if text.len() >= 4096 {
            break;
        }
    }

    Some((status, String::from_utf8_lossy(&text).to_string()))
}
