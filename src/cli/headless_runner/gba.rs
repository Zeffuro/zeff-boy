use self::test_status::{GbaTestResult, GbaTestStatus, read_gba_test_status};
use super::*;

mod test_status;

pub(super) fn run_gba_headless(
    path: &Path,
    rom_data: &[u8],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options("gba", opts)?;

    let mut emulator = GbaEmulator::from_rom_data(rom_data)?;
    let mut sram_recovery =
        crate::save_paths::battery_sram_session(path, "gba", emulator.rom_hash());
    if opts.gba_hidden_bg_layers.iter().any(|&hidden| hidden) {
        emulator.set_ppu_debug_bg_layers(std::array::from_fn(|i| !opts.gba_hidden_bg_layers[i]));
    }
    if opts.gba_hide_sprites {
        let bg_enabled = opts.gba_hidden_bg_layers.iter().any(|&hidden| !hidden);
        emulator.set_ppu_debug_flags(bg_enabled, true, false);
        if opts.gba_hidden_bg_layers.iter().any(|&hidden| hidden) {
            emulator
                .set_ppu_debug_bg_layers(std::array::from_fn(|i| !opts.gba_hidden_bg_layers[i]));
        }
    }
    if !opts.no_sram
        && let Some(sram_path) = crate::emu_backend::gba::try_load_battery_sram(&mut emulator, path)
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
    if opts.no_apu {
        emulator.set_apu_sample_generation_enabled(false);
    }
    if opts.gba_audio_mutes.iter().any(|&muted| muted) {
        emulator.set_apu_channel_mutes(opts.gba_audio_mutes);
        log::info!(
            "Applied GBA audio channel mutes: {:?}",
            opts.gba_audio_mutes
        );
    }

    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let mut screenshot_written = false;
    let mut traced = 0u64;
    let mut bus_traced = 0u64;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let bus_trace_active = !opts.trace_bus_filters.is_empty();
    let trace_gba_bus_reads = opts.trace_bus_filters.iter().any(|filter| {
        matches!(
            filter.access,
            HeadlessBusTraceAccess::Read | HeadlessBusTraceAccess::ReadWrite
        )
    });
    let trace_gba_bus_writes = opts.trace_bus_filters.iter().any(|filter| {
        matches!(
            filter.access,
            HeadlessBusTraceAccess::Write | HeadlessBusTraceAccess::ReadWrite
        )
    });
    let mut current_input = InputMasks::default();
    let start = Instant::now();
    let mut frames_run = 0u64;
    let mut audio_scratch: Vec<f32> = Vec::new();
    let mut audio_dump: Vec<f32> = Vec::new();
    let mut audio_stats = AudioStats::default();
    let mut test_pass_seen = false;
    let mut test_status_seen = false;
    let mut last_test_status: Option<GbaTestStatus> = None;
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

    'gba_frames: for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        if current_input.reset {
            emulator.reset();
        }
        emulator.set_input(current_input.buttons, current_input.dpad);

        if opts.trace_opcodes || bus_trace_active || opts.break_on_gba_bad_state {
            emulator.clear_frame_ready();
            let guard = emulator
                .cpu_cycles()
                .wrapping_add(u64::from(GBA_CYCLES_PER_FRAME) * 2);
            while !emulator.frame_ready() && emulator.cpu_cycles() < guard {
                let before_cycles = emulator.cpu_cycles();
                let bus_trace_collecting = bus_trace_active
                    && (opts.trace_bus_limit == 0 || bus_traced < opts.trace_bus_limit);
                let (fetched, bus_events) = if bus_trace_collecting {
                    emulator
                        .step_instruction_with_bus_trace(trace_gba_bus_reads, trace_gba_bus_writes)
                } else {
                    (emulator.step_instruction(), Vec::new())
                };
                let Some(fetched) = fetched else {
                    if emulator.is_cpu_suspended() {
                        break;
                    }
                    continue;
                };
                let step_cycles = emulator.cpu_cycles().wrapping_sub(before_cycles);
                if opts.trace_opcodes && emulator.cpu_cycles() >= opts.trace_start_t {
                    let tail_line = format_gba_op_tail_line(&emulator, fetched, step_cycles);
                    if tail.len() == 64 {
                        tail.pop_front();
                    }
                    tail.push_back(tail_line);
                }
                if opts.trace_opcodes
                    && (opts.trace_opcode_limit == 0 || traced < opts.trace_opcode_limit)
                    && should_trace_gba_op(opts, fetched.pc, emulator.cpu_cycles())
                {
                    let op_line = format_gba_op_line(traced, &emulator, fetched, step_cycles);
                    println!("{}", op_line);
                    traced = traced.wrapping_add(1);
                }
                if opts.break_on_gba_bad_state
                    && let Some(reason) = gba_bad_state_reason(&emulator, fetched)
                {
                    println!(
                        "[headless] system=gba bad-state frame={} cycles={} pc={} reason={}",
                        frame_number,
                        emulator.cpu_cycles(),
                        format_pc(u64::from(fetched.pc), 8),
                        reason
                    );
                    frames_run = frame_number;
                    break 'gba_frames;
                }
                if bus_trace_collecting && emulator.cpu_cycles() >= opts.trace_start_t {
                    for event in bus_events {
                        if opts.trace_bus_limit != 0 && bus_traced >= opts.trace_bus_limit {
                            break;
                        }
                        if should_trace_gba_bus_event(opts, event) {
                            println!(
                                "{}",
                                format_gba_bus_trace_line(bus_traced, &emulator, fetched, event)
                            );
                            bus_traced = bus_traced.wrapping_add(1);
                        }
                    }
                }
            }
            emulator.finish_frame();
        } else {
            emulator.step_frame();
        }
        frames_run = frame_number;
        check_tas_assertions(
            opts,
            frames_run,
            emulator.cpu_pc(),
            emulator.framebuffer(),
            || emulator.encode_state(),
        )?;

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

        if opts.expect_test_pass
            && let Some(status) = read_gba_test_status(&emulator)
        {
            test_status_seen = true;
            last_test_status = Some(status.clone());
            match status.result {
                GbaTestResult::Pass => {
                    println!(
                        "[headless] gba-test result=pass protocol={} status={:02X} text={:?}",
                        status.protocol, status.code, status.text
                    );
                    test_pass_seen = true;
                    break;
                }
                GbaTestResult::Fail => {
                    anyhow::bail!(
                        "GBA test failed via {} status={:02X} text={:?}",
                        status.protocol,
                        status.code,
                        status.text
                    );
                }
                GbaTestResult::Running => {}
            }
        }

        let wait_classification = gba_wait_classification(&emulator);
        observe_stuck(
            &mut stuck,
            "gba",
            8,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
            None,
            wait_classification,
            wait_classification.is_some(),
            &mut stuck_active,
        );

        if !opts.no_apu {
            emulator.drain_audio_samples_into(&mut audio_scratch);
            audio_stats.observe(&audio_scratch);
            if opts.audio_dump_path.is_some() {
                audio_dump.extend_from_slice(&audio_scratch);
            }
            audio_scratch.clear();
        }

        if emulator.is_cpu_suspended() {
            println!(
                "[headless] system=gba suspended frame={} cycles={} pc={}",
                frames_run,
                emulator.cpu_cycles(),
                format_pc(u64::from(emulator.cpu_pc()), 8)
            );
            break;
        }
    }
    ensure_tas_completed(opts, frames_run)?;

    if opts.trace_opcodes {
        println!("[gba-op-tail] ---- last {} ops ----", tail.len());
        for line in tail {
            println!("{}", line);
        }
    }

    println!(
        "[headless] system=gba frames={} cycles={} pc={} title={:?} backup={:?}",
        frames_run,
        emulator.cpu_cycles(),
        format_pc(u64::from(emulator.cpu_pc()), 8),
        emulator.cartridge_header().title,
        emulator.backup_kind()
    );
    print_perf("gba", frames_run, start);
    write_final_screenshot_if_needed(
        opts,
        frames_run,
        emulator.framebuffer(),
        emulator.framebuffer_dimensions(),
        &mut screenshot_written,
    )?;
    emit_debug_state(
        opts,
        gba_debug_state(
            &emulator,
            frames_run,
            opts,
            current_input,
            stuck.as_ref().and_then(StuckTracker::current_report),
            screenshot_path_if_written(opts, screenshot_written),
            audio_stats,
        ),
    )?;
    if let Some(dir) = &opts.gba_dump_memory_dir {
        dump_gba_memory_snapshots(&emulator, dir)?;
    }
    if let Some(path) = &opts.audio_dump_path {
        write_audio_dump_f32le(path, &audio_dump, emulator.apu_debug_snapshot().sample_rate)?;
    }
    if !opts.no_sram {
        flush_battery(
            &mut sram_recovery,
            path,
            ActiveSystem::Gba,
            emulator.rom_hash(),
            emulator.dump_battery_sram(),
        );
    }
    fail_on_stuck_if_needed("gba", stuck.as_ref(), opts)?;

    if opts.expect_test_pass && !test_pass_seen {
        if test_status_seen {
            if let Some(status) = last_test_status {
                anyhow::bail!(
                    "expected GBA test pass before max frame limit, last protocol={} status={:02X} text={:?}",
                    status.protocol,
                    status.code,
                    status.text
                );
            }
        } else {
            anyhow::bail!(
                "expected GBA test pass before max frame limit, but no supported GBA test status was observed"
            );
        }
    }

    Ok(())
}
