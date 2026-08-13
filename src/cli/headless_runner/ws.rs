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
    if opts.ws_link_peer_path.is_some() {
        return run_ws_link_pair_headless(path, rom_data, opts);
    }
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

#[derive(Default)]
struct WsLinkPairStats {
    left_to_right_tx: u64,
    right_to_left_tx: u64,
    max_cycle_skew: u64,
}

const WS_SERIAL_STATUS_RX_READY: u8 = 0x01;
const WS_SERIAL_STATUS_OVERRUN: u8 = 0x02;

fn run_ws_link_pair_headless(
    path: &Path,
    rom_data: &[u8],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_ws_link_pair_options(opts)?;

    let peer_path = opts
        .ws_link_peer_path
        .as_ref()
        .expect("caller checked ws_link_peer_path");
    let (peer_rom_path, peer_rom_data) = if peer_path
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case("same")
    {
        (path.to_path_buf(), rom_data.to_vec())
    } else {
        let (peer_rom_path, peer_rom_data, peer_system) = load_headless_rom(peer_path)?;
        if peer_system != ActiveSystem::WonderSwan {
            anyhow::bail!("--ws-link-peer must point to a WonderSwan/WSC ROM, got {peer_system:?}");
        }
        (peer_rom_path, peer_rom_data)
    };

    let mut left = WsEmulator::new(rom_data, zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE)?;
    let mut right = WsEmulator::new(&peer_rom_data, zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE)?;

    if !opts.no_sram {
        load_ws_battery_if_present(&mut left, path, "left");
        load_ws_battery_if_present(&mut right, &peer_rom_path, "right");
    }
    if opts.no_apu {
        left.set_apu_sample_generation_enabled(false);
        right.set_apu_sample_generation_enabled(false);
    }

    let start = Instant::now();
    let mut frames_run = 0u64;
    let mut stats = WsLinkPairStats::default();
    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        let left_input = input_for_frame(opts, frame_number);
        let right_input = input_p2_for_frame(opts, frame_number);
        left.set_input(left_input.buttons, left_input.dpad);
        right.set_input(right_input.buttons, right_input.dpad);

        step_linked_ws_frame_pair(&mut left, &mut right, &mut stats);
        frames_run = frame_number;

        if left.is_cpu_suspended() || right.is_cpu_suspended() {
            break;
        }
    }

    let left_uart = left.uart_debug_snapshot();
    let right_uart = right.uart_debug_snapshot();
    println!(
        "[headless] ws-link frames={} left_cycles={} right_cycles={} left_pc={:06X} right_pc={:06X}",
        frames_run,
        left.cpu_cycles(),
        right.cpu_cycles(),
        left.cpu_pc(),
        right.cpu_pc()
    );
    println!(
        "[headless] ws-link bytes left_to_right={} right_to_left={} max_cycle_skew={}",
        stats.left_to_right_tx, stats.right_to_left_tx, stats.max_cycle_skew
    );
    println!(
        "[headless] ws-link uart left_status={:02X} right_status={:02X} left_rx_ready={} right_rx_ready={} left_overrun={} right_overrun={}",
        left_uart.status,
        right_uart.status,
        left_uart.status & WS_SERIAL_STATUS_RX_READY != 0,
        right_uart.status & WS_SERIAL_STATUS_RX_READY != 0,
        left_uart.status & WS_SERIAL_STATUS_OVERRUN != 0,
        right_uart.status & WS_SERIAL_STATUS_OVERRUN != 0
    );
    print_perf("ws-link", frames_run, start);

    if left.is_cpu_suspended() || right.is_cpu_suspended() {
        println!(
            "[headless] ws-link cpu-suspended left={} right={} left_trap={:?} right_trap={:?}",
            left.is_cpu_suspended(),
            right.is_cpu_suspended(),
            left.last_trap(),
            right.last_trap()
        );
    }

    if opts.expect_ws_link_bytes != 0 {
        let expected = opts.expect_ws_link_bytes;
        if stats.left_to_right_tx < expected || stats.right_to_left_tx < expected {
            anyhow::bail!(
                "WonderSwan link exchanged too few bytes: left_to_right={} right_to_left={} expected_each_direction>={}",
                stats.left_to_right_tx,
                stats.right_to_left_tx,
                expected
            );
        }
    }

    Ok(())
}

fn ensure_ws_link_pair_options(opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.expect_ws_text.is_some()
        || opts.expect_ws_pass_fail_tiles
        || opts.trace_opcodes
        || !opts.trace_bus_filters.is_empty()
        || opts.trace_watch_interrupts
        || opts.load_state_path.is_some()
        || opts.screenshot_path.is_some()
        || opts.screenshot_frame.is_some()
        || opts.screenshot_dir.is_some()
        || opts.print_debug_state
        || opts.debug_state_path.is_some()
        || opts.audio_dump_path.is_some()
        || !opts.memory_dumps.is_empty()
    {
        anyhow::bail!(
            "--ws-link-peer is currently a minimal link diagnostic mode; combine it only with --max-frames, --press, --press-p2, --input-script, --input-script-p2, --no-apu, --no-sram, and --expect-ws-link-bytes"
        );
    }
    Ok(())
}

fn load_ws_battery_if_present(emulator: &mut WsEmulator, path: &Path, label: &str) {
    if let Some(sram_path) = crate::emu_backend::ws::try_load_battery_sram(emulator, path)
        .unwrap_or_else(|err| {
            log::warn!("Failed to load {label} battery save: {err}");
            None
        })
    {
        log::info!("Loaded {label} battery save from {}", sram_path);
    }
}

fn step_linked_ws_frame_pair(
    left: &mut WsEmulator,
    right: &mut WsEmulator,
    stats: &mut WsLinkPairStats,
) {
    use zeff_ws_core::hardware::constants::CYCLES_PER_FRAME;

    let left_was_suspended = left.is_cpu_suspended();
    let right_was_suspended = right.is_cpu_suspended();
    let left_guard = left
        .cpu_cycles()
        .wrapping_add(u64::from(CYCLES_PER_FRAME) * 2);
    let right_guard = right
        .cpu_cycles()
        .wrapping_add(u64::from(CYCLES_PER_FRAME) * 2);

    if !left_was_suspended {
        left.clear_frame_ready();
    }
    if !right_was_suspended {
        right.clear_frame_ready();
    }

    while should_step_ws_frame_side(left, left_was_suspended, left_guard)
        || should_step_ws_frame_side(right, right_was_suspended, right_guard)
    {
        if should_step_ws_frame_side(left, left_was_suspended, left_guard) {
            left.step_instruction();
        }
        drain_ws_link_events(left, right, &mut stats.left_to_right_tx);

        if should_step_ws_frame_side(right, right_was_suspended, right_guard) {
            right.step_instruction();
        }
        drain_ws_link_events(right, left, &mut stats.right_to_left_tx);

        stats.max_cycle_skew = stats
            .max_cycle_skew
            .max(left.cpu_cycles().abs_diff(right.cpu_cycles()));
    }

    drain_ws_link_events(left, right, &mut stats.left_to_right_tx);
    drain_ws_link_events(right, left, &mut stats.right_to_left_tx);

    if !left_was_suspended {
        left.finish_frame();
    }
    if !right_was_suspended {
        right.finish_frame();
    }
}

fn should_step_ws_frame_side(emulator: &WsEmulator, was_suspended: bool, guard_cycle: u64) -> bool {
    !was_suspended
        && !emulator.is_cpu_suspended()
        && !emulator.frame_ready()
        && emulator.cpu_cycles() < guard_cycle
}

fn drain_ws_link_events(source: &mut WsEmulator, target: &mut WsEmulator, count: &mut u64) {
    while let Some(event) = source.take_wonder_swan_link_tx_event() {
        *count = count.wrapping_add(1);
        target.receive_wonder_swan_link_byte(event.byte);
    }
}
