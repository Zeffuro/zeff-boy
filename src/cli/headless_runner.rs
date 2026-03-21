use std::collections::VecDeque;
use std::path::Path;

use crate::emulator::Emulator;
use crate::hardware::types::hardware_mode::HardwareModePreference;

use super::output::{
    format_headless_breakpoint, format_headless_serial, format_headless_summary, format_op_line,
    format_op_tail_line,
};
use super::trace_filters::{ime_short, mode_short, should_trace_op};
use super::types::HeadlessOptions;

pub(crate) fn run_headless(
    path: &Path,
    mode_preference: HardwareModePreference,
    opts: &HeadlessOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut emulator = Emulator::from_rom_with_mode(path, mode_preference)?;
    if let Some(addr) = opts.break_at {
        emulator.debug.add_breakpoint(addr);
    }
    let mut traced = 0u64;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(64);

    for _ in 0..opts.max_frames {
        if opts.trace_opcodes {
            let target = emulator
                .cpu
                .cycles
                .wrapping_add(Emulator::cycles_per_frame());
            while emulator.cpu.cycles < target {
                let (pc, op, cb_prefix, step_cycles) = emulator.step_instruction();
                if matches!(
                    emulator.cpu.running,
                    crate::hardware::types::CPUState::Suspended
                ) {
                    println!(
                        "{}",
                        format_headless_breakpoint(
                            emulator.cpu.pc,
                            emulator.cpu.cycles,
                            emulator.cpu.a,
                            emulator.cpu.f,
                            emulator.cpu.sp,
                        )
                    );
                    return Ok(());
                }
                let if_reg = emulator.bus.if_reg;
                let ie = emulator.bus.ie;
                let ime = &emulator.cpu.ime;
                if (opts.trace_opcode_limit == 0 || traced < opts.trace_opcode_limit)
                    && should_trace_op(opts, pc, op, emulator.cpu.cycles, ime, if_reg, ie)
                {
                    let pending = (if_reg & ie) & 0x1F;
                    let op1 = emulator.bus.read_byte(pc.wrapping_add(1));
                    let op2 = emulator.bus.read_byte(pc.wrapping_add(2));
                    let mut op_extra = String::new();
                    if !cb_prefix {
                        match op {
                            0xFA => {
                                let addr = u16::from_le_bytes([op1, op2]);
                                let value = emulator.bus.read_byte(addr);
                                op_extra = format!(" fa_addr={:04X} fa_val={:02X}", addr, value);
                            }
                            0xF0 => {
                                let addr = 0xFF00u16 | u16::from(op1);
                                let value = emulator.bus.read_byte(addr);
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
                    let zf = (emulator.cpu.f >> 7) & 1;
                    let nf = (emulator.cpu.f >> 6) & 1;
                    let hf = (emulator.cpu.f >> 5) & 1;
                    let cf = (emulator.cpu.f >> 4) & 1;

                    let op_line = format_op_line(
                        traced,
                        pc,
                        op,
                        cb_prefix,
                        step_cycles,
                        emulator.cpu.cycles,
                        ime_short(ime),
                        if_reg,
                        ie,
                        pending,
                        emulator.bus.io.timer.div,
                        emulator.bus.io.timer.tima,
                        emulator.bus.io.timer.tac,
                        emulator.cpu.a,
                        emulator.cpu.f,
                        zf,
                        nf,
                        hf,
                        cf,
                        mode_short(emulator.bus.hardware_mode),
                        &op_extra,
                    );
                    println!("{}", op_line);

                    traced = traced.wrapping_add(1);
                    let tail_line = format_op_tail_line(
                        pc,
                        op,
                        cb_prefix,
                        step_cycles,
                        emulator.cpu.cycles,
                        ime_short(ime),
                        if_reg,
                        ie,
                        pending,
                        emulator.bus.io.timer.div,
                        emulator.bus.io.timer.tima,
                        emulator.bus.io.timer.tac,
                        emulator.cpu.a,
                        emulator.cpu.f,
                        zf,
                        nf,
                        hf,
                        cf,
                        mode_short(emulator.bus.hardware_mode),
                        &op_extra,
                    );
                    if tail.len() == 64 {
                        tail.pop_front();
                    }
                    tail.push_back(tail_line);
                }
            }
        } else {
            emulator.step_frame();
            if matches!(
                emulator.cpu.running,
                crate::hardware::types::CPUState::Suspended
            ) {
                println!(
                    "{}",
                    format_headless_breakpoint(
                        emulator.cpu.pc,
                        emulator.cpu.cycles,
                        emulator.cpu.a,
                        emulator.cpu.f,
                        emulator.cpu.sp,
                    )
                );
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

    let serial_bytes = emulator.bus.io.serial.output_bytes();
    let serial_text = String::from_utf8_lossy(serial_bytes);
    println!(
        "{}",
        format_headless_summary(
            opts.max_frames,
            emulator.cpu.cycles,
            emulator.cpu.pc,
            serial_bytes.len()
        )
    );
    if !serial_text.is_empty() {
        println!("{}", format_headless_serial(&serial_text));
    }

    if let Some(expected) = &opts.expect_serial {
        if !serial_text.contains(expected) {
            return Err(format!(
                "expected serial output containing {:?}, got {:?}",
                expected, serial_text
            )
            .into());
        }
    }

    Ok(())
}
