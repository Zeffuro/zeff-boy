use std::collections::VecDeque;
use std::path::Path;

use zeff_gb_core::emulator::Emulator;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::output::{
    TraceContext, format_headless_breakpoint, format_headless_serial, format_headless_summary,
    format_op_line, format_op_tail_line,
};
use super::trace_filters::{ime_short, mode_short, should_trace_op};
use super::types::HeadlessOptions;

pub(crate) fn run_headless(
    path: &Path,
    mode_preference: HardwareModePreference,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let rom_data = std::fs::read(path)?;
    let mut emulator = Emulator::from_rom_data(&rom_data, mode_preference)?;
    if let Some(sram_path) = crate::emu_backend::gb::try_load_battery_sram(&mut emulator, path)
        .unwrap_or_else(|e| { log::warn!("Failed to load battery save: {e}"); None })
    {
        log::info!("Loaded battery save from {}", sram_path);
    }
    if opts.no_apu {
        emulator.set_apu_enabled(false);
        emulator.set_apu_sample_generation_enabled(false);
        log::info!("APU disabled for profiling");
    }
    let flush_battery = |emulator: &Emulator| {
        if let Some(bytes) = emulator.dump_battery_sram() {
            let save_path = path.with_extension("sav");
            match crate::save_paths::write_sram_file(&save_path, &bytes) {
                Ok(()) => log::info!("Saved battery RAM to {}", save_path.display()),
                Err(err) => log::error!("Failed to save battery RAM: {}", err),
            }
        }
    };
    if let Some(addr) = opts.break_at {
        emulator.add_breakpoint(addr);
    }
    let mut traced = 0u64;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);

    for _ in 0..opts.max_frames {
        if opts.trace_opcodes {
            let target = emulator
                .cpu_cycles()
                .wrapping_add(Emulator::cycles_per_frame(emulator.hardware_mode()));
            while emulator.cpu_cycles() < target {
                let (pc, op, cb_prefix, step_cycles) = emulator.step_instruction();
                if emulator.is_cpu_suspended() {
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
                    flush_battery(&emulator);
                    return Ok(());
                }
                let if_reg = emulator.if_reg();
                let ie = emulator.ie_reg();
                let ime = emulator.cpu_ime();
                if (opts.trace_opcode_limit == 0 || traced < opts.trace_opcode_limit)
                    && should_trace_op(opts, pc, op, emulator.cpu_cycles(), &ime, if_reg, ie)
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
            if emulator.is_cpu_suspended() {
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
                flush_battery(&emulator);
                return Ok(());
            }
        }
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
            opts.max_frames,
            emulator.cpu_cycles(),
            emulator.cpu_pc(),
            serial_bytes.len()
        )
    );
    if !serial_text.is_empty() {
        println!("{}", format_headless_serial(&serial_text));
    }

    if let Some(expected) = &opts.expect_serial
        && !serial_text.contains(expected) {
            flush_battery(&emulator);
            anyhow::bail!(
                "expected serial output containing {:?}, got {:?}",
                expected, serial_text
            );
        }

    flush_battery(&emulator);

    Ok(())
}
