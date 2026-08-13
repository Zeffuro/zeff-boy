use zeff_ws_core::emulator::Emulator as WsEmulator;
use zeff_ws_core::hardware::bus::DebugTraceEvent as WsBusTraceEvent;
use zeff_ws_core::hardware::cpu::FetchedInstruction as WsFetchedInstruction;

use crate::cli::types::{HeadlessBusTraceAccess, HeadlessOptions};

pub(in crate::cli::headless_runner) fn should_trace_ws_op(
    opts: &HeadlessOptions,
    pc: u32,
    op: u8,
    total_t: u64,
) -> bool {
    if total_t < opts.trace_start_t {
        return false;
    }

    if let Some((start, end)) = opts.trace_pc_range
        && (u64::from(pc) < start || u64::from(pc) > end)
    {
        return false;
    }

    if !opts.trace_opcode_filter.is_empty() && !opts.trace_opcode_filter.contains(&op) {
        return false;
    }

    true
}

pub(in crate::cli::headless_runner) fn should_trace_ws_bus_event(
    opts: &HeadlessOptions,
    event: WsBusTraceEvent,
) -> bool {
    let (addr, is_read) = match event {
        WsBusTraceEvent::Read { addr, .. } => (u64::from(addr), true),
        WsBusTraceEvent::Write { addr, .. } => (u64::from(addr), false),
        WsBusTraceEvent::IoRead { port, .. } => (u64::from(port), true),
        WsBusTraceEvent::IoWrite { port, .. } => (u64::from(port), false),
    };

    opts.trace_bus_filters.iter().any(|filter| {
        addr >= filter.start_addr
            && addr <= filter.end_addr
            && matches!(
                (filter.access, is_read),
                (HeadlessBusTraceAccess::ReadWrite, _)
                    | (HeadlessBusTraceAccess::Read, true)
                    | (HeadlessBusTraceAccess::Write, false)
            )
    })
}

pub(in crate::cli::headless_runner) fn format_ws_bus_trace_line(
    traced: u64,
    emulator: &WsEmulator,
    fetched: Option<WsFetchedInstruction>,
    event: WsBusTraceEvent,
) -> String {
    let access = match event {
        WsBusTraceEvent::Read { addr, value } => {
            format!("read addr={addr:05X} value={value:02X}")
        }
        WsBusTraceEvent::Write {
            addr,
            old_value,
            new_value,
        } => {
            format!("write addr={addr:05X} old={old_value:02X} new={new_value:02X}")
        }
        WsBusTraceEvent::IoRead { port, value } => {
            format!("ioread port={port:04X} value={value:02X}")
        }
        WsBusTraceEvent::IoWrite {
            port,
            written_value,
            old_value,
            new_value,
        } => {
            format!(
                "iowrite port={port:04X} write={written_value:02X} old={old_value:02X} new={new_value:02X}"
            )
        }
    };
    let pc = fetched.map_or_else(|| emulator.cpu_pc(), |fetch| fetch.pc);
    let op = fetched.map_or_else(|| emulator.cpu_last_opcode(), |fetch| fetch.opcode);
    format!(
        "[ws-bus] n={} pc={:05X} op={:02X} total_t={} ivb={:02X} ie={:02X} irq={:02X} keys={:02X} {}",
        traced,
        pc,
        op,
        emulator.cpu_cycles(),
        emulator.io_peek8(0xB0),
        emulator.io_peek8(0xB2),
        emulator.io_peek8(0xB4),
        emulator.io_peek8(0xB5),
        access,
    )
}

pub(in crate::cli::headless_runner) fn format_ws_op_line(
    traced: u64,
    emulator: &WsEmulator,
    fetched: WsFetchedInstruction,
    step_cycles: u64,
) -> String {
    format!(
        "[ws-op] n={} {}",
        traced,
        format_ws_op_fields(emulator, fetched, step_cycles)
    )
}

pub(in crate::cli::headless_runner) fn format_ws_op_tail_line(
    emulator: &WsEmulator,
    fetched: WsFetchedInstruction,
    step_cycles: u64,
) -> String {
    format!(
        "[ws-op-tail] {}",
        format_ws_op_fields(emulator, fetched, step_cycles)
    )
}

fn format_ws_op_fields(
    emulator: &WsEmulator,
    fetched: WsFetchedInstruction,
    step_cycles: u64,
) -> String {
    let regs = emulator.cpu_registers();
    let segs = emulator.cpu_segments();
    let ppu = emulator.ppu_debug_snapshot();
    format!(
        "pc={:05X} cs={:04X} ip={:04X} op={:02X} step_t={} total_t={} flags={:04X} ivb={:02X} ie={:02X} irq={:02X} keys={:02X} vcount={} line_t={} ax={:04X} cx={:04X} dx={:04X} bx={:04X} sp={:04X} bp={:04X} si={:04X} di={:04X} es={:04X} ds={:04X} ss={:04X} next_pc={:05X}",
        fetched.pc,
        fetched.cs,
        fetched.ip,
        fetched.opcode,
        step_cycles,
        emulator.cpu_cycles(),
        emulator.cpu_flags(),
        emulator.io_peek8(0xB0),
        emulator.io_peek8(0xB2),
        emulator.io_peek8(0xB4),
        emulator.io_peek8(0xB5),
        ppu.vcount,
        ppu.line_cycles,
        regs[0],
        regs[1],
        regs[2],
        regs[3],
        regs[4],
        regs[5],
        regs[6],
        regs[7],
        segs[0],
        segs[3],
        segs[2],
        emulator.cpu_pc(),
    )
}
