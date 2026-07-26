use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::bus::DebugTraceEvent as GbaBusTraceEvent;
use zeff_gba_core::hardware::cpu::{
    CpuMode, FetchedInstruction as GbaFetchedInstruction, InstructionSet,
};
use zeff_nes_core::emulator::Emulator as NesEmulator;
use zeff_nes_core::hardware::bus::DebugTraceEvent as NesBusTraceEvent;

use crate::cli::types::{HeadlessBusTraceAccess, HeadlessOptions};

pub(super) fn should_trace_nes_op(opts: &HeadlessOptions, pc: u16, op: u8, total_t: u64) -> bool {
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

pub(super) fn should_trace_gba_op(opts: &HeadlessOptions, pc: u32, total_t: u64) -> bool {
    if total_t < opts.trace_start_t {
        return false;
    }

    if let Some((start, end)) = opts.trace_pc_range
        && (u64::from(pc) < start || u64::from(pc) > end)
    {
        return false;
    }

    true
}

pub(super) fn gba_bad_state_reason(
    emulator: &GbaEmulator,
    fetched: GbaFetchedInstruction,
) -> Option<&'static str> {
    if fetched.instruction_set != InstructionSet::Thumb || emulator.cpu_mode() != CpuMode::Irq {
        return None;
    }

    if matches!(fetched.pc, 0x0000_0000..=0x0000_3FFF) {
        return Some("irq-thumb-bios-fetch");
    }

    if fetched.raw == 0 && matches!(fetched.pc, 0x0200_0000..=0x03FF_FFFF) {
        return Some("irq-thumb-zero-fetch");
    }

    if fetched.raw == 0xFFFF && !gba_executable_region(fetched.pc) {
        return Some("irq-thumb-open-bus-fetch");
    }

    None
}

fn gba_executable_region(pc: u32) -> bool {
    matches!(
        pc,
        0x0000_0000..=0x0000_3FFF
            | 0x0200_0000..=0x03FF_FFFF
            | 0x0800_0000..=0x0DFF_FFFF
    )
}

pub(super) fn should_trace_nes_bus_event(opts: &HeadlessOptions, event: NesBusTraceEvent) -> bool {
    let (addr, is_read) = match event {
        NesBusTraceEvent::Read { addr, .. } => (addr, true),
        NesBusTraceEvent::Write { addr, .. } => (addr, false),
    };

    opts.trace_bus_filters.iter().any(|filter| {
        u64::from(addr) >= filter.start_addr
            && u64::from(addr) <= filter.end_addr
            && match (filter.access, is_read) {
                (HeadlessBusTraceAccess::ReadWrite, _) => true,
                (HeadlessBusTraceAccess::Read, true) => true,
                (HeadlessBusTraceAccess::Write, false) => true,
                _ => false,
            }
    })
}

pub(super) fn should_trace_gba_bus_event(opts: &HeadlessOptions, event: GbaBusTraceEvent) -> bool {
    let (addr, is_read) = match event {
        GbaBusTraceEvent::Read { addr, .. } => (addr, true),
        GbaBusTraceEvent::Write { addr, .. } => (addr, false),
    };

    opts.trace_bus_filters.iter().any(|filter| {
        u64::from(addr) >= filter.start_addr
            && u64::from(addr) <= filter.end_addr
            && match (filter.access, is_read) {
                (HeadlessBusTraceAccess::ReadWrite, _) => true,
                (HeadlessBusTraceAccess::Read, true) => true,
                (HeadlessBusTraceAccess::Write, false) => true,
                _ => false,
            }
    })
}

pub(super) fn nes_op_extra(pc: u16, op: u8, op1: u8, op2: u8) -> Option<String> {
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

pub(super) fn format_gba_bus_trace_line(
    traced: u64,
    emulator: &GbaEmulator,
    fetched: GbaFetchedInstruction,
    event: GbaBusTraceEvent,
) -> String {
    let access = match event {
        GbaBusTraceEvent::Read { addr, value, width } => {
            let digits = usize::from(width) * 2;
            format!("read{} addr={addr:08X} value={value:0digits$X}", width * 8)
        }
        GbaBusTraceEvent::Write {
            addr,
            old_value,
            new_value,
            width,
        } => {
            let digits = usize::from(width) * 2;
            format!(
                "write{} addr={addr:08X} old={old_value:0digits$X} new={new_value:0digits$X}",
                width * 8
            )
        }
    };
    format!(
        "[gba-bus] n={} pc={:08X} raw={:08X} set={:?} total_t={} cpsr={:08X} mode={:?} {}",
        traced,
        fetched.pc,
        fetched.raw,
        fetched.instruction_set,
        emulator.cpu_cycles(),
        emulator.cpu_cpsr(),
        emulator.cpu_mode(),
        access,
    )
}

pub(super) fn format_nes_bus_trace_line(
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
pub(super) fn format_nes_op_line(
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

pub(super) fn format_gba_op_line(
    traced: u64,
    emulator: &GbaEmulator,
    fetched: GbaFetchedInstruction,
    step_cycles: u64,
) -> String {
    format!(
        "[gba-op] n={} {}",
        traced,
        format_gba_op_fields(emulator, fetched, step_cycles)
    )
}

pub(super) fn format_gba_op_tail_line(
    emulator: &GbaEmulator,
    fetched: GbaFetchedInstruction,
    step_cycles: u64,
) -> String {
    format!(
        "[gba-op-tail] {}",
        format_gba_op_fields(emulator, fetched, step_cycles)
    )
}

fn format_gba_op_fields(
    emulator: &GbaEmulator,
    fetched: GbaFetchedInstruction,
    step_cycles: u64,
) -> String {
    let regs = emulator.cpu_registers();
    let raw_width = if fetched.width_bytes == 2 { 4 } else { 8 };
    let ie = emulator.cpu_peek16(0x0400_0200);
    let if_reg = emulator.cpu_peek16(0x0400_0202);
    let ime = emulator.cpu_peek16(0x0400_0208);
    format!(
        "pc={:08X} raw={:0raw_width$X} set={:?} step_t={} total_t={} cpsr={:08X} mode={:?} ie={:04X} if={:04X} ime={:04X} r0={:08X} r1={:08X} r2={:08X} r3={:08X} r4={:08X} r5={:08X} r6={:08X} r7={:08X} r8={:08X} r9={:08X} r10={:08X} r11={:08X} r12={:08X} sp={:08X} lr={:08X} next_pc={:08X} decoded={:?}",
        fetched.pc,
        fetched.raw,
        fetched.instruction_set,
        step_cycles,
        emulator.cpu_cycles(),
        emulator.cpu_cpsr(),
        emulator.cpu_mode(),
        ie,
        if_reg,
        ime,
        regs[0],
        regs[1],
        regs[2],
        regs[3],
        regs[4],
        regs[5],
        regs[6],
        regs[7],
        regs[8],
        regs[9],
        regs[10],
        regs[11],
        regs[12],
        regs[13],
        regs[14],
        emulator.cpu_pc(),
        fetched.decoded
    )
}
