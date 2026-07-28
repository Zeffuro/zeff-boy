use super::*;

pub(super) fn run_gba_headless(
    path: &Path,
    rom_data: &[u8],
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    ensure_system_headless_options("gba", opts)?;

    let mut emulator = GbaEmulator::from_rom_data(rom_data)?;
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

        if opts.trace_opcodes || bus_trace_active {
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
        flush_battery(path, emulator.dump_battery_sram());
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

#[derive(Clone, Debug)]
struct GbaTestStatus {
    protocol: &'static str,
    code: u32,
    text: String,
    result: GbaTestResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GbaTestResult {
    Pass,
    Fail,
    Running,
}

fn read_gba_test_status(emulator: &GbaEmulator) -> Option<GbaTestStatus> {
    read_gba_memory_test_status(emulator).or_else(|| read_jsmolka_gba_screen_status(emulator))
}

fn read_gba_memory_test_status(emulator: &GbaEmulator) -> Option<GbaTestStatus> {
    const BASE: u32 = 0x0200_0000;
    const TEXT_LIMIT: u32 = 4096;

    let signature = [
        emulator.cpu_peek8(BASE + 1),
        emulator.cpu_peek8(BASE + 2),
        emulator.cpu_peek8(BASE + 3),
    ];
    if signature != [0xDE, 0xB0, 0x61] {
        return None;
    }

    let code = emulator.cpu_peek8(BASE);
    let mut text_bytes = Vec::new();
    for offset in 0..TEXT_LIMIT {
        let byte = emulator.cpu_peek8(BASE + 4 + offset);
        if byte == 0 {
            break;
        }
        text_bytes.push(byte);
    }
    let text = String::from_utf8_lossy(&text_bytes).to_string();
    let result = match code {
        0x00 => GbaTestResult::Pass,
        0x01..=0x7F => GbaTestResult::Fail,
        _ => GbaTestResult::Running,
    };

    Some(GbaTestStatus {
        protocol: "memory_status_02000000",
        code: u32::from(code),
        text,
        result,
    })
}

fn read_jsmolka_gba_screen_status(emulator: &GbaEmulator) -> Option<GbaTestStatus> {
    const TEXT_Y: usize = 76;
    const PASS_X: usize = 56;
    const FAIL_X: usize = 60;

    let vram = emulator.vram_snapshot();
    if gba_mode4_vram_matches_text(vram, PASS_X, TEXT_Y, "All tests passed") {
        return Some(GbaTestStatus {
            protocol: "jsmolka_mode4_text",
            code: 0,
            text: "All tests passed".to_string(),
            result: GbaTestResult::Pass,
        });
    }

    if gba_mode4_vram_matches_text(vram, FAIL_X, TEXT_Y, "Failed test ") {
        let digits_x = FAIL_X + "Failed test ".len() * 8;
        let digits = (0..3)
            .map(|digit| gba_mode4_vram_digit_at(vram, digits_x + digit * 8, TEXT_Y))
            .collect::<Option<String>>()
            .unwrap_or_else(|| "???".to_string());
        let code = digits.parse::<u32>().ok().unwrap_or(0x7FFF);
        return Some(GbaTestStatus {
            protocol: "jsmolka_mode4_text",
            code,
            text: format!("Failed test {digits}"),
            result: GbaTestResult::Fail,
        });
    }

    None
}

fn gba_mode4_vram_digit_at(vram: &[u8], x: usize, y: usize) -> Option<char> {
    ('0'..='9').find(|&digit| gba_mode4_vram_matches_char(vram, x, y, digit))
}

fn gba_mode4_vram_matches_text(vram: &[u8], x: usize, y: usize, text: &str) -> bool {
    text.chars()
        .enumerate()
        .all(|(index, ch)| gba_mode4_vram_matches_char(vram, x + index * 8, y, ch))
}

fn gba_mode4_vram_matches_char(vram: &[u8], x: usize, y: usize, ch: char) -> bool {
    let Some((upper, lower)) = gba_jsmolka_glyph_words(ch) else {
        return false;
    };

    if x + 8 > 240 || y + 8 > 160 {
        return false;
    }

    for py in 0..8 {
        let word = if py < 4 { upper } else { lower };
        let row = py % 4;
        for px in 0..8 {
            let bit = ((word >> (row * 8 + px)) & 1) as u8;
            let offset = (y + py) * 240 + x + px;
            if vram.get(offset).copied().unwrap_or(0) != bit {
                return false;
            }
        }
    }

    true
}

fn gba_jsmolka_glyph_words(ch: char) -> Option<(u32, u32)> {
    match ch {
        ' ' => Some((0x0000_0000, 0x0000_0000)),
        '0' => Some((0x7E76_663C, 0x003C_666E)),
        '1' => Some((0x181E_1C18, 0x0018_1818)),
        '2' => Some((0x3060_663C, 0x007E_0C18)),
        '3' => Some((0x3860_663C, 0x003C_6660)),
        '4' => Some((0x3336_3C38, 0x0030_307F)),
        '5' => Some((0x603E_067E, 0x003C_6660)),
        '6' => Some((0x3E06_0C38, 0x003C_6666)),
        '7' => Some((0x3060_607E, 0x0018_1818)),
        '8' => Some((0x3C66_663C, 0x003C_6666)),
        '9' => Some((0x7C66_663C, 0x001C_3060)),
        'A' => Some((0x7E66_663C, 0x0066_6666)),
        'F' => Some((0x1E06_067E, 0x0006_0606)),
        'a' => Some((0x603C_0000, 0x007C_667C)),
        'd' => Some((0x667C_6060, 0x007C_6666)),
        'e' => Some((0x663C_0000, 0x003C_067E)),
        'i' => Some((0x1818_0018, 0x0030_1818)),
        'l' => Some((0x1818_1818, 0x0030_1818)),
        'p' => Some((0x663E_0000, 0x0606_3E66)),
        's' => Some((0x063C_0000, 0x003E_603C)),
        't' => Some((0x0C3E_0C0C, 0x0038_0C0C)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{gba_jsmolka_glyph_words, gba_mode4_vram_matches_text};

    fn draw_char(vram: &mut [u8], x: usize, y: usize, ch: char) {
        let (upper, lower) = gba_jsmolka_glyph_words(ch).unwrap();
        for py in 0..8 {
            let word = if py < 4 { upper } else { lower };
            let row = py % 4;
            for px in 0..8 {
                let bit = ((word >> (row * 8 + px)) & 1) as u8;
                vram[(y + py) * 240 + x + px] = bit;
            }
        }
    }

    fn draw_text(vram: &mut [u8], x: usize, y: usize, text: &str) {
        for (index, ch) in text.chars().enumerate() {
            draw_char(vram, x + index * 8, y, ch);
        }
    }

    #[test]
    fn jsmolka_mode4_text_match_finds_pass_string() {
        let mut vram = vec![0; 0x18000];
        draw_text(&mut vram, 56, 76, "All tests passed");
        assert!(gba_mode4_vram_matches_text(
            &vram,
            56,
            76,
            "All tests passed"
        ));
        assert!(!gba_mode4_vram_matches_text(&vram, 60, 76, "Failed test "));
    }
}
