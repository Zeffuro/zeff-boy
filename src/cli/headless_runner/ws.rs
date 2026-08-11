use self::helpers::{
    WsPassFailTileStats, compact_ws_text, print_ws_memory_dumps, ws_background_screen_text,
    ws_pass_fail_tile_stats, ws_progress_marker, ws_wait_classification,
};
use super::*;

mod helpers;

pub(super) fn run_ws_headless(
    path: &Path,
    rom_data: &[u8],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options("ws", opts)?;
    ensure_no_reset_events("ws", opts)?;
    if opts.trace_watch_interrupts {
        anyhow::bail!("WonderSwan --trace-watch-interrupts is not implemented yet");
    }
    if opts.expect_test_pass {
        anyhow::bail!(
            "--expect-test-pass is not implemented for WonderSwan headless runs yet; use --expect-ws-text or --expect-ws-pass-fail-tiles"
        );
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
    let mut traced = 0u64;
    let mut bus_traced = 0u64;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
    let bus_trace_active = !opts.trace_bus_filters.is_empty();
    let start = Instant::now();
    let mut frames_run = 0u64;
    let mut current_input = InputMasks::default();
    let mut audio_scratch: Vec<f32> = Vec::new();
    let mut audio_dump: Vec<f32> = Vec::new();
    let mut audio_stats = AudioStats::default();
    let expect_ws_text = opts.expect_ws_text.as_deref();
    let mut ws_text_pass_seen = expect_ws_text.is_none();
    let mut last_ws_text = String::new();
    let expect_ws_pass_fail_tiles = opts.expect_ws_pass_fail_tiles;
    let mut pass_fail_tile_stats = WsPassFailTileStats::default();
    let mut first_fail_tile_frame = None;
    let mut cpu_suspended = false;
    write_screenshot_if_requested(
        opts,
        0,
        emulator.framebuffer(),
        dimensions,
        &mut screenshot_written,
    )?;
    write_screenshot_sequence_if_requested(opts, 0, emulator.framebuffer(), dimensions)?;
    emulator.set_opcode_log_enabled(
        opts.trace_opcodes || opts.print_debug_state || opts.debug_state_path.is_some(),
    );

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        emulator.set_input(current_input.buttons, current_input.dpad);

        if opts.trace_opcodes || bus_trace_active {
            emulator.clear_frame_ready();
            let guard = emulator
                .cpu_cycles()
                .wrapping_add(u64::from(zeff_ws_core::hardware::constants::CYCLES_PER_FRAME) * 2);
            while !emulator.frame_ready()
                && emulator.cpu_cycles() < guard
                && !emulator.is_cpu_suspended()
            {
                let before_cycles = emulator.cpu_cycles();
                let bus_trace_collecting = bus_trace_active
                    && (opts.trace_bus_limit == 0 || bus_traced < opts.trace_bus_limit);
                let (fetched, bus_events) = if bus_trace_collecting {
                    emulator.step_instruction_with_bus_trace()
                } else {
                    (emulator.step_instruction(), Vec::new())
                };
                let step_cycles = emulator.cpu_cycles().wrapping_sub(before_cycles);

                if let Some(fetched) = fetched {
                    if opts.trace_opcodes && emulator.cpu_cycles() >= opts.trace_start_t {
                        let tail_line = format_ws_op_tail_line(&emulator, fetched, step_cycles);
                        if tail.len() == 64 {
                            tail.pop_front();
                        }
                        tail.push_back(tail_line);
                    }
                    if opts.trace_opcodes
                        && (opts.trace_opcode_limit == 0 || traced < opts.trace_opcode_limit)
                        && should_trace_ws_op(
                            opts,
                            fetched.pc,
                            fetched.opcode,
                            emulator.cpu_cycles(),
                        )
                    {
                        println!(
                            "{}",
                            format_ws_op_line(traced, &emulator, fetched, step_cycles)
                        );
                        traced = traced.wrapping_add(1);
                    }
                }

                if bus_trace_collecting && emulator.cpu_cycles() >= opts.trace_start_t {
                    for event in bus_events {
                        if opts.trace_bus_limit != 0 && bus_traced >= opts.trace_bus_limit {
                            break;
                        }
                        if should_trace_ws_bus_event(opts, event) {
                            println!(
                                "{}",
                                format_ws_bus_trace_line(
                                    bus_traced,
                                    &emulator,
                                    fetched.or_else(|| emulator.last_fetch()),
                                    event,
                                )
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

        if let Some(expected) = expect_ws_text {
            last_ws_text = ws_background_screen_text(&emulator);
            if last_ws_text.contains("Failed") || last_ws_text.contains("FAILED") {
                anyhow::bail!(
                    "WonderSwan screen text reported failure before finding {expected:?}:\n{}",
                    compact_ws_text(&last_ws_text)
                );
            }
            if last_ws_text.contains(expected) {
                println!(
                    "[headless] ws-test result=pass text_contains={expected:?} frame={frames_run}"
                );
                ws_text_pass_seen = true;
                break;
            }
        }
        if expect_ws_pass_fail_tiles {
            pass_fail_tile_stats = ws_pass_fail_tile_stats(&emulator);
            if pass_fail_tile_stats.fail_tiles > 0 && first_fail_tile_frame.is_none() {
                first_fail_tile_frame = Some(frames_run);
            }
        }

        let wait_classification = ws_wait_classification(&emulator);
        observe_stuck(
            &mut stuck,
            "ws",
            6,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
            ws_progress_marker(&emulator),
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
                "[headless] ws cpu-suspended frame={} pc={:06X} trap={:?}",
                frames_run,
                emulator.cpu_pc(),
                emulator.last_trap()
            );
            cpu_suspended = true;
            break;
        }
    }

    if opts.trace_opcodes {
        println!("[ws-op-tail] ---- last {} ops ----", tail.len());
        for line in tail {
            println!("{}", line);
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
    emit_debug_state(
        opts,
        ws_debug_state(WsDebugStateRequest {
            emulator: &emulator,
            frames_run,
            opts,
            input: current_input,
            stuck: stuck.as_ref().and_then(StuckTracker::current_report),
            screenshot: screenshot_path_if_written(opts, screenshot_written),
            audio_stats,
        }),
    )?;
    if !opts.no_sram {
        flush_battery(path, emulator.dump_battery_sram());
    }
    if let Some(path) = &opts.audio_dump_path {
        write_audio_dump_f32le(path, &audio_dump, emulator.apu_debug_snapshot().sample_rate)?;
        println!(
            "[headless] ws-audio drained_samples={} drained_frames={} nonzero_samples={} peak_abs={:.6} mean_abs={:.6}",
            audio_stats.sample_count,
            audio_stats.frames_with_samples,
            audio_stats.nonzero_samples,
            audio_stats.peak_abs,
            audio_stats.mean_abs()
        );
    }
    print_ws_memory_dumps(&emulator, opts);
    fail_on_stuck_if_needed("ws", stuck.as_ref(), opts)?;
    if cpu_suspended && expect_ws_text.is_some() {
        anyhow::bail!(
            "WonderSwan CPU suspended before expected screen text was found: pc={:06X} trap={:?}",
            emulator.cpu_pc(),
            emulator.last_trap()
        );
    }
    if let Some(expected) = expect_ws_text
        && !ws_text_pass_seen
    {
        anyhow::bail!(
            "WonderSwan screen text did not contain {expected:?} within {} frames; last decoded text:\n{}",
            opts.max_frames,
            compact_ws_text(&last_ws_text)
        );
    }
    if expect_ws_pass_fail_tiles {
        if pass_fail_tile_stats.fail_tiles > 0 {
            let text = if last_ws_text.is_empty() {
                ws_background_screen_text(&emulator)
            } else {
                last_ws_text
            };
            anyhow::bail!(
                "WonderSwan pass/fail tilemap contained {} fail marker(s) and {} pass marker(s) at frame {}; first fail frame={:?}; decoded text:\n{}",
                pass_fail_tile_stats.fail_tiles,
                pass_fail_tile_stats.pass_tiles,
                frames_run,
                first_fail_tile_frame,
                compact_ws_text(&text)
            );
        }
        if pass_fail_tile_stats.pass_tiles == 0 {
            let text = if last_ws_text.is_empty() {
                ws_background_screen_text(&emulator)
            } else {
                last_ws_text
            };
            anyhow::bail!(
                "WonderSwan pass/fail tilemap did not contain any pass markers within {} frames; decoded text:\n{}",
                opts.max_frames,
                compact_ws_text(&text)
            );
        }
        println!(
            "[headless] ws-test result=pass pass_tiles={} frame={frames_run}",
            pass_fail_tile_stats.pass_tiles
        );
    }

    Ok(())
}
