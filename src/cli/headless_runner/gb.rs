use super::*;

const GB_MEMORY_TEST_TEXT_FAST_SCAN: u16 = 512;
const GB_MEMORY_TEST_TEXT_FULL_SCAN: u16 = 0x1FFC;

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
    if let Some(preset) = opts.gb_dmg_palette_preset {
        emulator.set_dmg_palette_preset(preset);
        log::info!("Applied DMG palette preset {}", preset.label());
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
    let mut test_pass_seen = false;
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

    'frames: for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        emulator.set_input(current_input.buttons, current_input.dpad);

        if opts.trace_opcodes || opts.expect_test_pass {
            let target = emulator
                .cpu_cycles()
                .wrapping_add(GbEmulator::cycles_per_frame(emulator.hardware_mode()));
            while emulator.cpu_cycles() < target {
                let (pc, op, cb_prefix, step_cycles) = emulator.step_instruction();
                if let Some(result) = check_breakpoint(&emulator) {
                    return result;
                }
                if opts.expect_test_pass {
                    if let Some(status) =
                        read_gb_memory_test_status(&emulator, GB_MEMORY_TEST_TEXT_FAST_SCAN)
                    {
                        match status.code {
                            0x00 if status.text.to_ascii_lowercase().contains("passed") => {
                                test_pass_seen = true;
                                break;
                            }
                            0x01..=0x7F => {
                                flush_battery(&emulator);
                                print_gb_memory_dumps(&emulator, opts);
                                anyhow::bail!(
                                    "memory-status test failure code {}: {}",
                                    status.code,
                                    status.text
                                );
                            }
                            _ => {}
                        }
                    }
                    match test_pass_breakpoint_result(&emulator, pc, op) {
                        Some(TestPassResult::Pass) => {
                            test_pass_seen = true;
                            break;
                        }
                        Some(TestPassResult::Fail) => {
                            flush_battery(&emulator);
                            print_gb_memory_dumps(&emulator, opts);
                            anyhow::bail!(
                                "test failure breakpoint at PC={:04X}: B={:02X} C={:02X} D={:02X} E={:02X} H={:02X} L={:02X}",
                                pc,
                                emulator.cpu_b(),
                                emulator.cpu_c(),
                                emulator.cpu_d(),
                                emulator.cpu_e(),
                                emulator.cpu_h(),
                                emulator.cpu_l(),
                            );
                        }
                        None => {}
                    }
                }
                let if_reg = emulator.if_reg();
                let ie = emulator.ie_reg();
                let ime = emulator.cpu_ime();
                if opts.trace_opcodes
                    && (opts.trace_opcode_limit == 0 || traced < opts.trace_opcode_limit)
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
                        ppu_cycles: emulator.ppu_cycles(),
                        ppu_lcdc: emulator.ppu_lcdc(),
                        ppu_stat: emulator.ppu_stat(),
                        ppu_ly: emulator.ppu_ly(),
                        ppu_lyc: emulator.ppu_lyc(),
                        a: emulator.cpu_a(),
                        f,
                        b: emulator.cpu_b(),
                        c: emulator.cpu_c(),
                        d: emulator.cpu_d(),
                        e: emulator.cpu_e(),
                        h: emulator.cpu_h(),
                        l: emulator.cpu_l(),
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
            if opts.expect_test_pass
                && !test_pass_seen
                && let Some(status) =
                    read_gb_memory_test_status(&emulator, GB_MEMORY_TEST_TEXT_FULL_SCAN)
            {
                match status.code {
                    0x00 if status.text.to_ascii_lowercase().contains("passed") => {
                        test_pass_seen = true;
                    }
                    0x01..=0x7F => {
                        flush_battery(&emulator);
                        print_gb_memory_dumps(&emulator, opts);
                        anyhow::bail!(
                            "memory-status test failure code {}: {}",
                            status.code,
                            status.text
                        );
                    }
                    _ => {}
                }
            }
            if opts.expect_test_pass
                && !test_pass_seen
                && let Some((result, screen_text)) = gb_screen_test_pass_result(&emulator)
            {
                match result {
                    TestPassResult::Pass => {
                        test_pass_seen = true;
                    }
                    TestPassResult::Fail => {
                        flush_battery(&emulator);
                        print_gb_memory_dumps(&emulator, opts);
                        anyhow::bail!("screen test failure: {}", screen_text);
                    }
                }
            }
            if opts.expect_test_pass && !test_pass_seen {
                let serial_text = String::from_utf8_lossy(emulator.serial_output_bytes());
                match serial_test_pass_result(&serial_text) {
                    Some(TestPassResult::Pass) => {
                        test_pass_seen = true;
                    }
                    Some(TestPassResult::Fail) => {
                        flush_battery(&emulator);
                        print_gb_memory_dumps(&emulator, opts);
                        anyhow::bail!("serial test failure: {}", serial_text);
                    }
                    None => {}
                }
            }
            if test_pass_seen {
                frames_run = frame_number;
                break 'frames;
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

    if opts.expect_test_pass
        && !test_pass_seen
        && matches!(
            serial_test_pass_result(&serial_text),
            Some(TestPassResult::Pass)
        )
    {
        test_pass_seen = true;
    }

    if opts.expect_test_pass
        && !test_pass_seen
        && matches!(
            gb_screen_test_pass_result(&emulator).map(|(result, _)| result),
            Some(TestPassResult::Pass)
        )
    {
        test_pass_seen = true;
    }

    if opts.expect_test_pass && !test_pass_seen {
        flush_battery(&emulator);
        print_gb_memory_dumps(&emulator, opts);
        anyhow::bail!(
            "expected test pass breakpoint B=03 C=05 D=08 E=0D H=15 L=22 before max frame limit, got final B={:02X} C={:02X} D={:02X} E={:02X} H={:02X} L={:02X}",
            emulator.cpu_b(),
            emulator.cpu_c(),
            emulator.cpu_d(),
            emulator.cpu_e(),
            emulator.cpu_h(),
            emulator.cpu_l(),
        );
    }

    flush_battery(&emulator);
    print_gb_memory_dumps(&emulator, opts);
    fail_on_stuck_if_needed("gb", stuck.as_ref(), opts)?;

    Ok(())
}

fn print_gb_memory_dumps(emulator: &GbEmulator, opts: &HeadlessOptions) {
    for dump in &opts.memory_dumps {
        let start = dump.start_addr;
        let len = dump.len;
        println!("[mem] start={:04X} len={}", start, len);
        let mut offset = 0u16;
        while offset < len {
            let line_len = (len - offset).min(16);
            let addr = start.wrapping_add(offset);
            let bytes = (0..line_len)
                .map(|i| format!("{:02X}", emulator.peek_byte_raw(addr.wrapping_add(i))))
                .collect::<Vec<_>>()
                .join(" ");
            println!("[mem] {:04X}: {}", addr, bytes);
            offset += line_len;
        }
    }
}

#[derive(Debug, Clone)]
struct GbMemoryTestStatus {
    code: u8,
    text: String,
}

fn read_gb_memory_test_status(
    emulator: &GbEmulator,
    text_limit: u16,
) -> Option<GbMemoryTestStatus> {
    let signature = [
        emulator.peek_byte_raw(0xA001),
        emulator.peek_byte_raw(0xA002),
        emulator.peek_byte_raw(0xA003),
    ];
    if signature != [0xDE, 0xB0, 0x61] {
        return None;
    }

    let mut text_bytes = Vec::new();
    for offset in 0..text_limit {
        let byte = emulator.peek_byte_raw(0xA004u16.wrapping_add(offset));
        if byte == 0 {
            break;
        }
        text_bytes.push(byte);
    }

    Some(GbMemoryTestStatus {
        code: emulator.peek_byte_raw(0xA000),
        text: String::from_utf8_lossy(&text_bytes).to_string(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestPassResult {
    Pass,
    Fail,
}

fn serial_test_pass_result(serial_text: &str) -> Option<TestPassResult> {
    let lower = serial_text.to_ascii_lowercase();
    if lower.contains("failed") || lower.contains("error") {
        Some(TestPassResult::Fail)
    } else if lower.contains("passed") {
        Some(TestPassResult::Pass)
    } else {
        None
    }
}

fn gb_screen_test_pass_result(emulator: &GbEmulator) -> Option<(TestPassResult, String)> {
    for tilemap_base in [0x9800u16, 0x9C00] {
        let text = read_gb_tilemap_ascii(emulator, tilemap_base);
        if let Some(result) = serial_test_pass_result(&text) {
            return Some((result, text));
        }
    }
    None
}

fn read_gb_tilemap_ascii(emulator: &GbEmulator, tilemap_base: u16) -> String {
    let mut text = String::with_capacity(32 * 32 + 31);
    for row in 0..32u16 {
        if row != 0 {
            text.push('\n');
        }
        for col in 0..32u16 {
            let tile = emulator.peek_byte_raw(tilemap_base + row * 32 + col);
            let ch = if tile.is_ascii_graphic() || tile == b' ' {
                tile as char
            } else {
                ' '
            };
            text.push(ch);
        }
    }
    text
}

fn test_pass_breakpoint_result(emulator: &GbEmulator, pc: u16, op: u8) -> Option<TestPassResult> {
    if !matches!(op, 0x40 | 0xED) || pc < 0x0100 {
        return None;
    }

    let regs = [
        emulator.cpu_b(),
        emulator.cpu_c(),
        emulator.cpu_d(),
        emulator.cpu_e(),
        emulator.cpu_h(),
        emulator.cpu_l(),
    ];

    if regs == [3, 5, 8, 13, 21, 34] {
        Some(TestPassResult::Pass)
    } else if regs == [0x42; 6] {
        Some(TestPassResult::Fail)
    } else {
        None
    }
}
