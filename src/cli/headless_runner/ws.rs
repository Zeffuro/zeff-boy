use super::*;

pub(super) fn run_ws_headless(
    path: &Path,
    rom_data: &[u8],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options("ws", opts)?;
    ensure_no_reset_events("ws", opts)?;
    if opts.trace_opcodes
        || !opts.trace_bus_filters.is_empty()
        || !opts.trace_opcode_filter.is_empty()
        || opts.trace_watch_interrupts
    {
        anyhow::bail!("WonderSwan headless tracing is not implemented yet");
    }
    if opts.expect_test_pass {
        anyhow::bail!("--expect-test-pass is not implemented for WonderSwan headless runs yet");
    }

    let mut emulator = WsEmulator::new(rom_data, zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE)?;
    if !opts.no_sram
        && let Some(sram_path) = crate::emu_backend::ws::try_load_battery_sram(&mut emulator, path)
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

    let dimensions = emulator.framebuffer_dimensions();
    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let mut screenshot_written = false;
    let start = Instant::now();
    let mut frames_run = 0u64;
    write_screenshot_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        dimensions,
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(opts, 0, emulator.framebuffer(), dimensions)?;

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        let input = input_for_frame(opts, frame_number);
        emulator.set_input(input.buttons, input.dpad);
        emulator.step_frame();
        frames_run = frame_number;

        write_screenshot_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            dimensions,
            &mut screenshot_written,
        )?;
        write_screenshot_sequence_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            dimensions,
        )?;

        observe_stuck(
            &mut stuck,
            "ws",
            6,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
            None,
            false,
            &mut stuck_active,
        );
        if emulator.is_cpu_suspended() {
            println!(
                "[headless] ws cpu-suspended frame={} pc={:06X} trap={:?}",
                frames_run,
                emulator.cpu_pc(),
                emulator.last_trap()
            );
            break;
        }
    }

    println!(
        "[headless] system=ws frames={} cycles={} pc={:06X}",
        frames_run,
        emulator.cpu_cycles(),
        emulator.cpu_pc()
    );
    print_perf("ws", frames_run, start);
    write_final_screenshot_if_needed(
        opts,
        frames_run,
        emulator.framebuffer(),
        dimensions,
        &mut screenshot_written,
    )?;
    if !opts.no_sram {
        flush_battery(path, emulator.dump_battery_sram());
    }
    fail_on_stuck_if_needed("ws", stuck.as_ref(), opts)?;

    Ok(())
}
