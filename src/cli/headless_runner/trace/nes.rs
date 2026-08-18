use zeff_nes_core::emulator::Emulator as NesEmulator;
use zeff_nes_core::hardware::bus::DebugTraceEvent as NesBusTraceEvent;

use crate::cli::types::{HeadlessBusTraceAccess, HeadlessOptions};

pub(in crate::cli::headless_runner) fn should_trace_nes_op(
    opts: &HeadlessOptions,
    pc: u16,
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

pub(in crate::cli::headless_runner) fn should_trace_nes_bus_event(
    opts: &HeadlessOptions,
    event: NesBusTraceEvent,
) -> bool {
    let (addr, is_read) = match event {
        NesBusTraceEvent::Read { addr, .. } => (addr, true),
        NesBusTraceEvent::Write { addr, .. } => (addr, false),
    };

    opts.trace_bus_filters.iter().any(|filter| {
        u64::from(addr) >= filter.start_addr
            && u64::from(addr) <= filter.end_addr
            && matches!(
                (filter.access, is_read),
                (HeadlessBusTraceAccess::ReadWrite, _)
                    | (HeadlessBusTraceAccess::Read, true)
                    | (HeadlessBusTraceAccess::Write, false)
            )
    })
}

pub(in crate::cli::headless_runner) fn nes_op_extra(
    pc: u16,
    op: u8,
    op1: u8,
    op2: u8,
) -> Option<String> {
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

pub(in crate::cli::headless_runner) fn format_nes_bus_trace_line(
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
            mapped_addr,
            ..
        } => {
            format!(
                "read addr={addr:04X} value={value:02X}{}",
                nes_ppu_addr_trace_suffix(mapped_addr.map(|addr| addr as u16))
            )
        }
        NesBusTraceEvent::Write {
            addr,
            old_value,
            new_value,
            mapped_addr,
            ..
        } => {
            format!(
                "write addr={addr:04X} old={old_value:02X} new={new_value:02X}{}",
                nes_ppu_addr_trace_suffix(mapped_addr.map(|addr| addr as u16))
            )
        }
    };
    format!(
        "[nes-bus] n={} pc={:04X} op={:02X} total_t={} ppu={}:{} ppustat={:02X} ppuctrl={:02X} ppumask={:02X} apu_fc={} apu_frd={} apu_mode={} apu_irq_inh={} apu_status={:02X} {}",
        traced,
        pc,
        op,
        emulator.cpu_cycles(),
        emulator.ppu_scanline(),
        emulator.ppu_dot(),
        emulator.ppu_status(),
        emulator.ppu_ctrl(),
        emulator.ppu_mask(),
        emulator.bus().apu.frame_cycle,
        emulator.bus().apu.frame_reset_delay,
        if emulator.bus().apu.five_step_mode {
            5
        } else {
            4
        },
        u8::from(emulator.bus().apu.irq_inhibit),
        emulator.bus().apu.peek_status(),
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
pub(in crate::cli::headless_runner) fn format_nes_op_line(
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
