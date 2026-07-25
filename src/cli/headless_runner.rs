use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

use zeff_gb_core::emulator::Emulator as GbEmulator;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_nes_core::emulator::Emulator as NesEmulator;
use zeff_nes_core::hardware::bus::DebugTraceEvent as NesBusTraceEvent;

use crate::emu_backend::ActiveSystem;

use super::output::{
    TraceContext, format_headless_breakpoint, format_headless_serial, format_headless_summary,
    format_op_line, format_op_tail_line,
};
use super::trace_filters::{ime_short, mode_short, should_trace_op};
use super::types::{HeadlessBusTraceAccess, HeadlessOptions};

#[derive(Clone)]
struct StuckReport {
    frame: u64,
    window_frames: usize,
    unique_pcs: usize,
    framebuffer_changed: bool,
    first_pc: u64,
    last_pc: u64,
}

struct StuckTracker {
    window_frames: usize,
    pc_threshold: usize,
    pcs: VecDeque<u64>,
    framebuffer_hashes: VecDeque<u64>,
    current_report: Option<StuckReport>,
}

#[derive(Clone, Copy, Default)]
struct InputMasks {
    buttons: u8,
    dpad: u8,
    reset: bool,
}

impl StuckTracker {
    fn from_options(opts: &HeadlessOptions) -> Option<Self> {
        if opts.stuck_window_frames == 0 {
            return None;
        }

        Some(Self {
            window_frames: opts.stuck_window_frames.min(usize::MAX as u64) as usize,
            pc_threshold: opts.stuck_pc_threshold,
            pcs: VecDeque::new(),
            framebuffer_hashes: VecDeque::new(),
            current_report: None,
        })
    }

    fn observe(&mut self, frame: u64, pc: u64, framebuffer: &[u8]) -> Option<&StuckReport> {
        self.pcs.push_back(pc);
        self.framebuffer_hashes
            .push_back(framebuffer_fingerprint(framebuffer));

        while self.pcs.len() > self.window_frames {
            self.pcs.pop_front();
        }
        while self.framebuffer_hashes.len() > self.window_frames {
            self.framebuffer_hashes.pop_front();
        }

        if self.pcs.len() < self.window_frames {
            return self.current_report.as_ref();
        }

        let unique_pcs = self.pcs.iter().copied().collect::<HashSet<_>>().len();
        let framebuffer_changed = self
            .framebuffer_hashes
            .iter()
            .copied()
            .collect::<HashSet<_>>()
            .len()
            > 1;

        if unique_pcs <= self.pc_threshold && !framebuffer_changed {
            self.current_report = Some(StuckReport {
                frame,
                window_frames: self.window_frames,
                unique_pcs,
                framebuffer_changed,
                first_pc: self.pcs.front().copied().unwrap_or(pc),
                last_pc: pc,
            });
        } else {
            self.current_report = None;
        }

        self.current_report.as_ref()
    }

    fn current_report(&self) -> Option<&StuckReport> {
        self.current_report.as_ref()
    }
}

pub(crate) fn run_headless(
    path: &Path,
    mode_preference: HardwareModePreference,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let (rom_path, rom_data, system) = load_headless_rom(path)?;

    match system {
        ActiveSystem::GameBoy => run_gb_headless(&rom_path, &rom_data, mode_preference, opts),
        ActiveSystem::GameBoyAdvance => run_gba_headless(&rom_path, &rom_data, opts),
        ActiveSystem::Nes => run_nes_headless(&rom_path, &rom_data, opts),
    }
}

fn load_headless_rom(path: &Path) -> anyhow::Result<(PathBuf, Vec<u8>, ActiveSystem)> {
    let (rom_path, preloaded_data, system) = crate::app::detect_and_extract_rom(path)?;
    let rom_data = match preloaded_data {
        Some(data) => data,
        None => std::fs::read(path)?,
    };
    Ok((rom_path, rom_data, system))
}

fn run_gb_headless(
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
                        &super::trace_filters::CpuTraceState {
                            pc,
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
        observe_stuck(
            &mut stuck,
            "gb",
            4,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
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

fn run_gba_headless(path: &Path, rom_data: &[u8], opts: &HeadlessOptions) -> anyhow::Result<()> {
    ensure_system_headless_options("gba", opts)?;

    let mut emulator = GbaEmulator::from_rom_data(rom_data)?;
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

    for frame in 0..opts.max_frames {
        let frame_number = frame + 1;
        current_input = input_for_frame(opts, frame_number);
        if current_input.reset {
            emulator.reset();
        }
        emulator.set_input(current_input.buttons, current_input.dpad);

        emulator.step_frame();
        frames_run = frame_number;

        write_screenshot_if_requested(
            opts,
            frames_run,
            emulator.framebuffer(),
            emulator.framebuffer_dimensions(),
            &mut screenshot_written,
        )?;

        observe_stuck(
            &mut stuck,
            "gba",
            8,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
            &mut stuck_active,
        );

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
        ),
    )?;
    if !opts.no_sram {
        flush_battery(path, emulator.dump_battery_sram());
    }
    fail_on_stuck_if_needed("gba", stuck.as_ref(), opts)?;

    Ok(())
}

fn run_nes_headless(path: &Path, rom_data: &[u8], opts: &HeadlessOptions) -> anyhow::Result<()> {
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

        observe_stuck(
            &mut stuck,
            "nes",
            4,
            frames_run,
            u64::from(emulator.cpu_pc()),
            emulator.framebuffer(),
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

fn ensure_system_headless_options(system: &str, opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.expect_serial.is_some() {
        anyhow::bail!("--expect-serial is only supported for GB/GBC headless runs");
    }
    if opts.break_at.is_some() {
        anyhow::bail!("--break-at is only supported for GB/GBC headless runs");
    }

    let trace_options_used = opts.trace_opcodes
        || opts.trace_opcode_limit != HeadlessOptions::default().trace_opcode_limit
        || opts.trace_start_t != 0
        || opts.trace_pc_range.is_some()
        || !opts.trace_opcode_filter.is_empty()
        || opts.trace_watch_interrupts
        || !opts.trace_bus_filters.is_empty()
        || opts.trace_bus_limit != HeadlessOptions::default().trace_bus_limit;
    if system == "gba" && trace_options_used {
        anyhow::bail!("opcode tracing is only supported for GB/GBC headless runs");
    }
    if system == "nes" && opts.trace_watch_interrupts {
        anyhow::bail!("--trace-watch-interrupts is only supported for GB/GBC headless runs");
    }

    log::info!("Running {system} headless smoke test");
    Ok(())
}

fn should_trace_nes_op(opts: &HeadlessOptions, pc: u16, op: u8, total_t: u64) -> bool {
    if total_t < opts.trace_start_t {
        return false;
    }

    if let Some((start, end)) = opts.trace_pc_range
        && (pc < start || pc > end)
    {
        return false;
    }

    if !opts.trace_opcode_filter.is_empty() && !opts.trace_opcode_filter.contains(&op) {
        return false;
    }

    true
}

fn should_trace_nes_bus_event(opts: &HeadlessOptions, event: NesBusTraceEvent) -> bool {
    let (addr, is_read) = match event {
        NesBusTraceEvent::Read { addr, .. } => (addr, true),
        NesBusTraceEvent::Write { addr, .. } => (addr, false),
    };

    opts.trace_bus_filters.iter().any(|filter| {
        addr >= filter.start_addr
            && addr <= filter.end_addr
            && match (filter.access, is_read) {
                (HeadlessBusTraceAccess::ReadWrite, _) => true,
                (HeadlessBusTraceAccess::Read, true) => true,
                (HeadlessBusTraceAccess::Write, false) => true,
                _ => false,
            }
    })
}

fn nes_op_extra(pc: u16, op: u8, op1: u8, op2: u8) -> Option<String> {
    let imm16 = u16::from_le_bytes([op1, op2]);
    let relative = || pc.wrapping_add(2).wrapping_add((op1 as i8 as i16) as u16);

    match op {
        0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => {
            Some(format!(" branch={:04X}", relative()))
        }
        0x20 | 0x4C | 0x6C => Some(format!(" target={imm16:04X}")),
        0xA9 | 0xA2 | 0xA0 | 0xC9 | 0xE0 | 0xC0 | 0x69 | 0xE9 | 0x29 | 0x09 | 0x49 => {
            Some(format!(" imm={op1:02X}"))
        }
        0xAD | 0xAE | 0xAC | 0x8D | 0x8E | 0x8C | 0x2C | 0xCD | 0xEC | 0xCC => {
            Some(format!(" addr={imm16:04X}"))
        }
        _ => None,
    }
}

fn format_nes_bus_trace_line(
    traced: u64,
    emulator: &NesEmulator,
    pc: u16,
    op: u8,
    event: NesBusTraceEvent,
) -> String {
    let access = match event {
        NesBusTraceEvent::Read {
            addr,
            value,
            ppu_addr,
        } => {
            format!(
                "read addr={addr:04X} value={value:02X}{}",
                nes_ppu_addr_trace_suffix(ppu_addr)
            )
        }
        NesBusTraceEvent::Write {
            addr,
            old_value,
            new_value,
            ppu_addr,
        } => {
            format!(
                "write addr={addr:04X} old={old_value:02X} new={new_value:02X}{}",
                nes_ppu_addr_trace_suffix(ppu_addr)
            )
        }
    };
    format!(
        "[nes-bus] n={} pc={:04X} op={:02X} total_t={} ppu={}:{} ppustat={:02X} ppuctrl={:02X} ppumask={:02X} {}",
        traced,
        pc,
        op,
        emulator.cpu_cycles(),
        emulator.ppu_scanline(),
        emulator.ppu_dot(),
        emulator.ppu_status(),
        emulator.ppu_ctrl(),
        emulator.ppu_mask(),
        access,
    )
}

fn nes_ppu_addr_trace_suffix(ppu_addr: Option<u16>) -> String {
    let Some(addr) = ppu_addr else {
        return String::new();
    };

    format!(
        " ppu_addr={:04X} ppu_region={}",
        addr,
        nes_ppu_addr_region(addr)
    )
}

fn nes_ppu_addr_region(addr: u16) -> &'static str {
    match addr & 0x3FFF {
        0x0000..=0x1FFF => "chr",
        0x2000..=0x2FFF => "nametable",
        0x3000..=0x3EFF => "nametable_mirror",
        0x3F00..=0x3FFF => "palette",
        _ => "unknown",
    }
}

#[allow(clippy::too_many_arguments)]
fn format_nes_op_line(
    traced: u64,
    emulator: &NesEmulator,
    pc: u16,
    op: u8,
    op1: u8,
    op2: u8,
    step_cycles: u64,
    op_extra: &str,
) -> String {
    let p = emulator.cpu_status();
    format!(
        "[nes-op] n={} pc={:04X} op={:02X} op1={:02X} op2={:02X} step_t={} total_t={} a={:02X} x={:02X} y={:02X} sp={:02X} p={:02X} nvdizc={}{}{}{}{}{} nmi={} irq={} ppu={}:{} ppustat={:02X} ppuctrl={:02X} ppumask={:02X}{}",
        traced,
        pc,
        op,
        op1,
        op2,
        step_cycles,
        emulator.cpu_cycles(),
        emulator.cpu_a(),
        emulator.cpu_x(),
        emulator.cpu_y(),
        emulator.cpu_sp(),
        p,
        (p >> 7) & 1,
        (p >> 6) & 1,
        (p >> 3) & 1,
        (p >> 2) & 1,
        (p >> 1) & 1,
        p & 1,
        u8::from(emulator.cpu_nmi_pending()),
        u8::from(emulator.cpu_irq_line()),
        emulator.ppu_scanline(),
        emulator.ppu_dot(),
        emulator.ppu_status(),
        emulator.ppu_ctrl(),
        emulator.ppu_mask(),
        op_extra
    )
}

fn ensure_no_reset_events(system: &str, opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.input_events.iter().any(|event| event.reset) {
        anyhow::bail!("reset input events are not supported for {system} headless runs yet");
    }
    Ok(())
}

fn framebuffer_fingerprint(framebuffer: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

    let mut hash = FNV_OFFSET ^ framebuffer.len() as u64;
    let stride = (framebuffer.len() / 4096).max(1);
    for byte in framebuffer.iter().step_by(stride) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn observe_stuck(
    tracker: &mut Option<StuckTracker>,
    system: &str,
    pc_width: usize,
    frame: u64,
    pc: u64,
    framebuffer: &[u8],
    stuck_active: &mut bool,
) {
    let Some(tracker) = tracker.as_mut() else {
        return;
    };

    match tracker.observe(frame, pc, framebuffer) {
        Some(report) if !*stuck_active => {
            println!("{}", format_stuck_report(system, report, pc_width));
            *stuck_active = true;
        }
        None if *stuck_active => {
            println!("[headless] system={system} stuck-cleared frame={frame}");
            *stuck_active = false;
        }
        _ => {}
    }
}

fn input_for_frame(opts: &HeadlessOptions, frame: u64) -> InputMasks {
    let mut input = InputMasks::default();
    for event in &opts.input_events {
        if (event.start_frame..=event.end_frame).contains(&frame) {
            input.buttons |= event.buttons;
            input.dpad |= event.dpad;
        }
        if event.reset && frame == event.start_frame {
            input.reset = true;
        }
    }
    input
}

fn map_host_to_nes_byte(buttons: u8, dpad: u8) -> u8 {
    (buttons & 0x0F)
        | ((dpad & 0x04) << 2)
        | ((dpad & 0x08) << 2)
        | ((dpad & 0x02) << 5)
        | ((dpad & 0x01) << 7)
}

fn write_screenshot_if_requested(
    opts: &HeadlessOptions,
    frame: u64,
    framebuffer: &[u8],
    dimensions: (usize, usize),
    screenshot_written: &mut bool,
) -> anyhow::Result<()> {
    if *screenshot_written || opts.screenshot_frame != Some(frame) {
        return Ok(());
    }
    let Some(path) = &opts.screenshot_path else {
        return Ok(());
    };
    write_rgba_png(path, framebuffer, dimensions)?;
    *screenshot_written = true;
    println!("[headless] screenshot={} frame={}", path.display(), frame);
    Ok(())
}

fn write_final_screenshot_if_needed(
    opts: &HeadlessOptions,
    frame: u64,
    framebuffer: &[u8],
    dimensions: (usize, usize),
    screenshot_written: &mut bool,
) -> anyhow::Result<()> {
    if *screenshot_written {
        return Ok(());
    }
    let Some(path) = &opts.screenshot_path else {
        return Ok(());
    };
    if opts
        .screenshot_frame
        .is_some_and(|requested| requested <= frame)
    {
        return Ok(());
    }
    write_rgba_png(path, framebuffer, dimensions)?;
    *screenshot_written = true;
    println!("[headless] screenshot={} frame={}", path.display(), frame);
    Ok(())
}

fn write_rgba_png(
    path: &Path,
    framebuffer: &[u8],
    dimensions: (usize, usize),
) -> anyhow::Result<()> {
    let (width, height) = dimensions;
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| anyhow::anyhow!("screenshot dimensions overflow"))?;
    if framebuffer.len() != expected_len {
        anyhow::bail!(
            "framebuffer length mismatch for screenshot: got {}, expected {} for {}x{} RGBA",
            framebuffer.len(),
            expected_len,
            width,
            height
        );
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder.write_header()?;
    png_writer.write_image_data(framebuffer)?;
    Ok(())
}

fn screenshot_path_if_written(
    opts: &HeadlessOptions,
    screenshot_written: bool,
) -> Option<&PathBuf> {
    screenshot_written.then_some(opts.screenshot_path.as_ref()?)
}

fn emit_debug_state(opts: &HeadlessOptions, value: serde_json::Value) -> anyhow::Result<()> {
    if let Some(path) = &opts.debug_state_path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(&value)?)?;
        println!("[headless] debug-state={}", path.display());
    }
    if opts.print_debug_state {
        println!("[headless-debug] {}", serde_json::to_string(&value)?);
    }
    Ok(())
}

fn stuck_report_json(report: Option<&StuckReport>) -> serde_json::Value {
    match report {
        Some(report) => serde_json::json!({
            "detected": true,
            "frame": report.frame,
            "window_frames": report.window_frames,
            "unique_pcs": report.unique_pcs,
            "framebuffer_changed": report.framebuffer_changed,
            "first_pc": report.first_pc,
            "last_pc": report.last_pc,
        }),
        None => serde_json::json!({ "detected": false }),
    }
}

fn input_json(input: InputMasks) -> serde_json::Value {
    serde_json::json!({
        "buttons": input.buttons,
        "dpad": input.dpad,
        "reset": input.reset,
        "buttons_hex": format!("{:02X}", input.buttons),
        "dpad_hex": format!("{:02X}", input.dpad),
    })
}

fn input_schedule_json(opts: &HeadlessOptions) -> serde_json::Value {
    let events = opts
        .input_events
        .iter()
        .map(|event| {
            serde_json::json!({
                "start_frame": event.start_frame,
                "end_frame": event.end_frame,
                "buttons": event.buttons,
                "dpad": event.dpad,
                "reset": event.reset,
                "buttons_hex": format!("{:02X}", event.buttons),
                "dpad_hex": format!("{:02X}", event.dpad),
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "event_count": events.len(),
        "events": events,
    })
}

fn screenshot_json(path: Option<&PathBuf>) -> serde_json::Value {
    match path {
        Some(path) => serde_json::json!({ "written": true, "path": path.display().to_string() }),
        None => serde_json::json!({ "written": false }),
    }
}

fn hex_bytes(bytes: &[u8]) -> Vec<String> {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn decode_printable_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| match byte {
            b'\n' | b'\r' | b'\t' => byte as char,
            0x20..=0x7E => byte as char,
            _ => '.',
        })
        .collect()
}

fn nes_cpu_window(emulator: &NesEmulator, start: u16, len: usize) -> Vec<u8> {
    (0..len)
        .map(|offset| emulator.cpu_peek(start.wrapping_add(offset as u16)))
        .collect()
}

fn nes_blargg_output_json(emulator: &NesEmulator) -> serde_json::Value {
    let sig = [
        emulator.cpu_peek(0x6001),
        emulator.cpu_peek(0x6002),
        emulator.cpu_peek(0x6003),
    ];
    if sig != [0xDE, 0xB0, 0x61] {
        return serde_json::json!({
            "present": false,
            "signature": hex_bytes(&sig),
        });
    }

    let status = emulator.cpu_peek(0x6000);
    let mut text = Vec::new();
    for addr in 0x6004..=0x7FFF {
        let byte = emulator.cpu_peek(addr);
        if byte == 0 {
            break;
        }
        text.push(byte);
        if text.len() >= 4096 {
            break;
        }
    }

    serde_json::json!({
        "present": true,
        "status": status,
        "status_hex": format!("{status:02X}"),
        "running": status == 0x80,
        "result": if status <= 0x7F { Some(status) } else { None },
        "text": String::from_utf8_lossy(&text).to_string(),
        "text_ascii": decode_printable_ascii(&text),
        "text_len": text.len(),
    })
}

fn gb_debug_state(
    emulator: &GbEmulator,
    frames_run: u64,
    opts: &HeadlessOptions,
    input: InputMasks,
    stuck: Option<&StuckReport>,
    screenshot: Option<&PathBuf>,
) -> serde_json::Value {
    let serial_text = String::from_utf8_lossy(emulator.serial_output_bytes()).to_string();
    serde_json::json!({
        "system": "gb",
        "frames": frames_run,
        "cycles": emulator.cpu_cycles(),
        "pc": emulator.cpu_pc(),
        "pc_hex": format!("{:04X}", emulator.cpu_pc()),
        "sp": emulator.cpu_sp(),
        "sp_hex": format!("{:04X}", emulator.cpu_sp()),
        "a": emulator.cpu_a(),
        "f": emulator.cpu_f(),
        "hardware_mode": format!("{:?}", emulator.hardware_mode()),
        "cpu_state": format!("{:?}", emulator.cpu_running()),
        "ime": format!("{:?}", emulator.cpu_ime()),
        "if": emulator.if_reg(),
        "ie": emulator.ie_reg(),
        "timer": {
            "div": emulator.timer_div(),
            "tima": emulator.timer_tima(),
            "tac": emulator.timer_tac(),
        },
        "serial": {
            "bytes": emulator.serial_output_bytes().len(),
            "text": serial_text,
        },
        "input": input_json(input),
        "input_schedule": input_schedule_json(opts),
        "stuck": stuck_report_json(stuck),
        "screenshot": screenshot_json(screenshot),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}

fn gba_debug_state(
    emulator: &GbaEmulator,
    frames_run: u64,
    opts: &HeadlessOptions,
    input: InputMasks,
    stuck: Option<&StuckReport>,
    screenshot: Option<&PathBuf>,
) -> serde_json::Value {
    let last_fetch = emulator.last_fetch().map(|fetch| {
        serde_json::json!({
            "pc": fetch.pc,
            "pc_hex": format!("{:08X}", fetch.pc),
            "raw": fetch.raw,
            "raw_hex": format!("{:08X}", fetch.raw),
            "instruction_set": format!("{:?}", fetch.instruction_set),
            "width_bytes": fetch.width_bytes,
            "fetch_cycles": fetch.fetch_cycles,
            "decoded": format!("{:?}", fetch.decoded),
        })
    });
    serde_json::json!({
        "system": "gba",
        "frames": frames_run,
        "cycles": emulator.cpu_cycles(),
        "pc": emulator.cpu_pc(),
        "pc_hex": format!("{:08X}", emulator.cpu_pc()),
        "visible_pc": emulator.cpu_visible_pc(),
        "visible_pc_hex": format!("{:08X}", emulator.cpu_visible_pc()),
        "cpsr": emulator.cpu_cpsr(),
        "cpsr_hex": format!("{:08X}", emulator.cpu_cpsr()),
        "thumb": emulator.cpu_thumb_state(),
        "mode": format!("{:?}", emulator.cpu_mode()),
        "registers": emulator.cpu_registers(),
        "suspended": emulator.is_cpu_suspended(),
        "title": &emulator.cartridge_header().title,
        "game_code": &emulator.cartridge_header().game_code,
        "backup": format!("{:?}", emulator.backup_kind()),
        "last_fetch": last_fetch,
        "input": input_json(input),
        "input_schedule": input_schedule_json(opts),
        "stuck": stuck_report_json(stuck),
        "screenshot": screenshot_json(screenshot),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}

fn nes_debug_state(
    emulator: &mut NesEmulator,
    frames_run: u64,
    opts: &HeadlessOptions,
    input: InputMasks,
    stuck: Option<&StuckReport>,
    screenshot: Option<&PathBuf>,
) -> serde_json::Value {
    let palette = emulator.ppu_palette_ram().to_vec();
    let nametable = emulator.ppu_nametable_ram();
    let nametable_nonzero = nametable.iter().filter(|&&byte| byte != 0).count();
    let nametable_sample = nametable.iter().take(128).copied().collect::<Vec<_>>();
    let chr = emulator.chr_ram_snapshot();
    let chr_nonzero = chr.iter().filter(|&&byte| byte != 0).count();
    let chr_sample = chr.iter().take(128).copied().collect::<Vec<_>>();
    let internal_ram_sample = emulator
        .system_ram()
        .iter()
        .take(256)
        .copied()
        .collect::<Vec<_>>();
    let prg_ram_sample = nes_cpu_window(emulator, 0x6000, 256);
    let blargg = nes_blargg_output_json(emulator);

    serde_json::json!({
        "system": "nes",
        "frames": frames_run,
        "cycles": emulator.cpu_cycles(),
        "pc": emulator.cpu_pc(),
        "pc_hex": format!("{:04X}", emulator.cpu_pc()),
        "suspended": emulator.is_cpu_suspended(),
        "cpu": {
            "a": emulator.cpu_a(),
            "a_hex": format!("{:02X}", emulator.cpu_a()),
            "x": emulator.cpu_x(),
            "x_hex": format!("{:02X}", emulator.cpu_x()),
            "y": emulator.cpu_y(),
            "y_hex": format!("{:02X}", emulator.cpu_y()),
            "sp": emulator.cpu_sp(),
            "sp_hex": format!("{:02X}", emulator.cpu_sp()),
            "status": emulator.cpu_status(),
            "status_hex": format!("{:02X}", emulator.cpu_status()),
            "last_opcode": emulator.cpu_last_opcode(),
            "last_opcode_hex": format!("{:02X}", emulator.cpu_last_opcode()),
            "last_opcode_pc": emulator.last_opcode_pc(),
            "last_opcode_pc_hex": format!("{:04X}", emulator.last_opcode_pc()),
            "last_step_cycles": emulator.cpu_last_step_cycles(),
            "nmi_pending": emulator.cpu_nmi_pending(),
            "irq_line": emulator.cpu_irq_line(),
            "nmi_count": emulator.cpu_nmi_count(),
            "irq_count": emulator.cpu_irq_count(),
            "vectors": {
                "nmi": {
                    "lo": emulator.cpu_peek(0xFFFA),
                    "hi": emulator.cpu_peek(0xFFFB),
                    "addr": (emulator.cpu_peek(0xFFFA) as u16)
                        | ((emulator.cpu_peek(0xFFFB) as u16) << 8),
                    "addr_hex": format!(
                        "{:04X}",
                        (emulator.cpu_peek(0xFFFA) as u16)
                            | ((emulator.cpu_peek(0xFFFB) as u16) << 8)
                    ),
                },
                "reset": {
                    "lo": emulator.cpu_peek(0xFFFC),
                    "hi": emulator.cpu_peek(0xFFFD),
                    "addr": (emulator.cpu_peek(0xFFFC) as u16)
                        | ((emulator.cpu_peek(0xFFFD) as u16) << 8),
                    "addr_hex": format!(
                        "{:04X}",
                        (emulator.cpu_peek(0xFFFC) as u16)
                            | ((emulator.cpu_peek(0xFFFD) as u16) << 8)
                    ),
                },
                "irq": {
                    "lo": emulator.cpu_peek(0xFFFE),
                    "hi": emulator.cpu_peek(0xFFFF),
                    "addr": (emulator.cpu_peek(0xFFFE) as u16)
                        | ((emulator.cpu_peek(0xFFFF) as u16) << 8),
                    "addr_hex": format!(
                        "{:04X}",
                        (emulator.cpu_peek(0xFFFE) as u16)
                            | ((emulator.cpu_peek(0xFFFF) as u16) << 8)
                    ),
                },
            },
        },
        "memory": {
            "internal_ram_sample": internal_ram_sample,
            "internal_ram_sample_hex": hex_bytes(&internal_ram_sample),
            "cpu_6000_sample": prg_ram_sample,
            "cpu_6000_sample_hex": hex_bytes(&prg_ram_sample),
            "blargg": blargg,
        },
        "mapper": emulator.cartridge_header().mapper_label(),
        "mapper_effective": emulator.cartridge_effective_mapper_label(),
        "battery": emulator.has_battery(),
        "ppu": {
            "ctrl": emulator.ppu_ctrl(),
            "ctrl_hex": format!("{:02X}", emulator.ppu_ctrl()),
            "mask": emulator.ppu_mask(),
            "mask_hex": format!("{:02X}", emulator.ppu_mask()),
            "status": emulator.ppu_status(),
            "status_hex": format!("{:02X}", emulator.ppu_status()),
            "scanline": emulator.ppu_scanline(),
            "dot": emulator.ppu_dot(),
            "frame_count": emulator.ppu_frame_count(),
            "in_vblank": emulator.ppu_in_vblank(),
            "frame_ready": emulator.ppu_frame_ready(),
            "scroll_v": emulator.ppu_scroll_v(),
            "scroll_t": emulator.ppu_scroll_t(),
            "fine_x": emulator.ppu_fine_x(),
            "tall_sprites": emulator.ppu_tall_sprites(),
            "palette_ram": palette,
            "nametable_nonzero_bytes": nametable_nonzero,
            "nametable_sample": nametable_sample,
            "chr_visible_nonzero_bytes": chr_nonzero,
            "chr_visible_sample": chr_sample,
        },
        "input": input_json(input),
        "input_schedule": input_schedule_json(opts),
        "stuck": stuck_report_json(stuck),
        "screenshot": screenshot_json(screenshot),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}

fn format_pc(pc: u64, width: usize) -> String {
    format!("{:0width$X}", pc, width = width)
}

fn format_stuck_report(system: &str, report: &StuckReport, pc_width: usize) -> String {
    format!(
        "[headless] system={} stuck-detected frame={} window={} unique_pcs={} framebuffer_changed={} first_pc={} last_pc={}",
        system,
        report.frame,
        report.window_frames,
        report.unique_pcs,
        if report.framebuffer_changed { 1 } else { 0 },
        format_pc(report.first_pc, pc_width),
        format_pc(report.last_pc, pc_width)
    )
}

fn print_perf(system: &str, frames_run: u64, start: Instant) {
    let elapsed = start.elapsed();
    let fps = if elapsed.is_zero() {
        0.0
    } else {
        frames_run as f64 / elapsed.as_secs_f64()
    };
    println!(
        "[headless] system={} elapsed_ms={} fps={:.0}",
        system,
        elapsed.as_millis(),
        fps
    );
}

fn flush_battery(path: &Path, sram_bytes: Option<Vec<u8>>) {
    match crate::save_paths::flush_battery_sram(path, sram_bytes) {
        Ok(Some(save_path)) => log::info!("Saved battery RAM to {}", save_path),
        Ok(None) => {}
        Err(err) => log::error!("Failed to save battery RAM: {}", err),
    }
}

fn fail_on_stuck_if_needed(
    system: &str,
    tracker: Option<&StuckTracker>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if opts.fail_on_stuck
        && let Some(report) = tracker.and_then(StuckTracker::current_report)
    {
        anyhow::bail!(
            "{system} headless run detected a stuck window: {} frames, {} unique PCs",
            report.window_frames,
            report.unique_pcs
        );
    }
    Ok(())
}
