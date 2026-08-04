use super::*;

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

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        let input = input_for_frame(opts, frame_number);
        emulator.set_input(input.buttons, input.dpad);

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

#[derive(Clone, Copy, Debug, Default)]
struct WsPassFailTileStats {
    pass_tiles: usize,
    fail_tiles: usize,
}

fn ws_background_screen_text(emulator: &WsEmulator) -> String {
    const SCREEN_BASE_PORT: u16 = 0x07;
    const MAP_WIDTH: usize = 32;
    const MAP_HEIGHT: usize = 32;

    let map_base_words = usize::from(emulator.io_peek8(SCREEN_BASE_PORT) & 0x0F) << 10;
    let mut text = String::new();
    for row in 0..MAP_HEIGHT {
        let mut line = String::with_capacity(MAP_WIDTH);
        for col in 0..MAP_WIDTH {
            let word_index = map_base_words + row * MAP_WIDTH + col;
            let byte_addr = (word_index * 2) as u32;
            let tile = u16::from_le_bytes([
                emulator.cpu_peek8(byte_addr),
                emulator.cpu_peek8(byte_addr.wrapping_add(1)),
            ]);
            line.push(ws_tile_text_char(tile));
        }

        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(trimmed);
        }
    }
    text
}

fn ws_pass_fail_tile_stats(emulator: &WsEmulator) -> WsPassFailTileStats {
    const PASS_TILE: u16 = 5;
    const FAIL_TILE: u16 = 6;

    let mut stats = WsPassFailTileStats::default();
    for tile in ws_screen_1_tiles(emulator) {
        match tile & 0x01FF {
            PASS_TILE => stats.pass_tiles += 1,
            FAIL_TILE => stats.fail_tiles += 1,
            _ => {}
        }
    }
    stats
}

fn ws_screen_1_tiles(emulator: &WsEmulator) -> impl Iterator<Item = u16> + '_ {
    const SCREEN_BASE_PORT: u16 = 0x07;
    const MAP_WIDTH: usize = 32;
    const MAP_HEIGHT: usize = 32;

    let map_base_words = usize::from(emulator.io_peek8(SCREEN_BASE_PORT) & 0x0F) << 10;
    (0..MAP_HEIGHT).flat_map(move |row| {
        (0..MAP_WIDTH).map(move |col| {
            let word_index = map_base_words + row * MAP_WIDTH + col;
            let byte_addr = (word_index * 2) as u32;
            u16::from_le_bytes([
                emulator.cpu_peek8(byte_addr),
                emulator.cpu_peek8(byte_addr.wrapping_add(1)),
            ])
        })
    })
}

fn ws_tile_text_char(tile: u16) -> char {
    let byte = (tile & 0x00FF) as u8;
    if byte == 0 {
        ' '
    } else if byte.is_ascii_graphic() || byte == b' ' {
        char::from(byte)
    } else {
        '.'
    }
}

fn compact_ws_text(text: &str) -> String {
    let mut compact = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    const MAX_CHARS: usize = 1200;
    if compact.chars().count() > MAX_CHARS {
        compact = compact.chars().take(MAX_CHARS).collect();
        compact.push_str("\n...");
    }
    compact
}

fn print_ws_memory_dumps(emulator: &WsEmulator, opts: &HeadlessOptions) {
    for dump in &opts.memory_dumps {
        let start = u32::from(dump.start_addr);
        let len = u32::from(dump.len);
        println!("[mem] start={start:05X} len={len}");
        let mut offset = 0u32;
        while offset < len {
            let line_len = (len - offset).min(16);
            let addr = start.wrapping_add(offset);
            let bytes = (0..line_len)
                .map(|i| format!("{:02X}", emulator.cpu_peek8(addr.wrapping_add(i))))
                .collect::<Vec<_>>()
                .join(" ");
            println!("[mem] {addr:05X}: {bytes}");
            offset += line_len;
        }
    }
}

fn ws_wait_classification(emulator: &WsEmulator) -> Option<&'static str> {
    if emulator.cpu_state() == zeff_ws_core::hardware::cpu::CpuState::Halted
        && emulator.io_peek8(0xB2) != 0
    {
        Some("ws-halt-idle")
    } else if ws_framebuffer_has_visible_content(emulator.framebuffer()) {
        Some("ws-static-visible-frame")
    } else {
        None
    }
}

fn ws_framebuffer_has_visible_content(framebuffer: &[u8]) -> bool {
    let mut chunks = framebuffer.chunks_exact(4);
    let Some(first) = chunks.next() else {
        return false;
    };
    chunks.any(|pixel| pixel[..3] != first[..3])
}

fn ws_progress_marker(emulator: &WsEmulator) -> Option<u64> {
    let fetched = emulator.last_fetch()?;
    if fetched.pc != emulator.cpu_pc() || !ws_string_opcode(fetched.opcode) {
        return None;
    }

    let regs = emulator.cpu_registers();
    let segments = emulator.cpu_segments();
    let mut marker = 0xCBF2_9CE4_8422_2325u64;
    for value in [
        fetched.pc as u64,
        fetched.opcode as u64,
        regs[0] as u64,
        regs[1] as u64,
        regs[2] as u64,
        regs[6] as u64,
        regs[7] as u64,
        segments[0] as u64,
        segments[3] as u64,
        emulator.cpu_flags() as u64,
    ] {
        marker ^= value;
        marker = marker.wrapping_mul(0x0000_0100_0000_01B3);
    }
    Some(marker)
}

fn ws_string_opcode(opcode: u8) -> bool {
    matches!(
        opcode,
        0x6C..=0x6F | 0xA4..=0xA7 | 0xAA..=0xAF
    )
}
