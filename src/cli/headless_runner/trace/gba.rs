use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::bus::DebugTraceEvent as GbaBusTraceEvent;
use zeff_gba_core::hardware::cpu::{
    CpuMode, FetchedInstruction as GbaFetchedInstruction, InstructionSet,
};

use crate::cli::types::{HeadlessBusTraceAccess, HeadlessOptions};

pub(in crate::cli::headless_runner) fn should_trace_gba_op(
    opts: &HeadlessOptions,
    pc: u32,
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

    true
}

pub(in crate::cli::headless_runner) fn gba_bad_state_reason(
    emulator: &GbaEmulator,
    fetched: GbaFetchedInstruction,
) -> Option<&'static str> {
    if matches!(fetched.pc, 0x0000_0000..=0x0000_3FFF)
        && !matches!(fetched.pc, 0x0000_0128..=0x0000_013F)
        && emulator.cpu_mode() != CpuMode::Irq
    {
        return Some("unexpected-bios-fetch");
    }

    if !gba_executable_region(fetched.pc) {
        return Some("invalid-pc-fetch");
    }

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
            | 0x0500_0000..=0x07FF_FFFF
            | 0x0800_0000..=0x0DFF_FFFF
    )
}

pub(in crate::cli::headless_runner) fn should_trace_gba_bus_event(
    opts: &HeadlessOptions,
    event: GbaBusTraceEvent,
) -> bool {
    let (addr, is_read) = match event {
        GbaBusTraceEvent::Read { addr, .. } => (addr, true),
        GbaBusTraceEvent::Write { addr, .. } => (addr, false),
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

pub(in crate::cli::headless_runner) fn format_gba_bus_trace_line(
    traced: u64,
    emulator: &GbaEmulator,
    fetched: GbaFetchedInstruction,
    event: GbaBusTraceEvent,
) -> String {
    let access = match event {
        GbaBusTraceEvent::Read {
            addr, value, width, ..
        } => {
            let width = width as u8;
            let digits = usize::from(width) * 2;
            format!("read{} addr={addr:08X} value={value:0digits$X}", width * 8)
        }
        GbaBusTraceEvent::Write {
            addr,
            old_value,
            new_value,
            width,
            ..
        } => {
            let width = width as u8;
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

pub(in crate::cli::headless_runner) fn format_gba_op_line(
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

pub(in crate::cli::headless_runner) fn format_gba_op_tail_line(
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
    let waitcnt = emulator.cpu_peek16(0x0400_0204);
    let ime = emulator.cpu_peek16(0x0400_0208);
    format!(
        "pc={:08X} raw={:0raw_width$X} set={:?} step_t={} total_t={} cpsr={:08X} mode={:?} ie={:04X} if={:04X} waitcnt={:04X} ime={:04X} r0={:08X} r1={:08X} r2={:08X} r3={:08X} r4={:08X} r5={:08X} r6={:08X} r7={:08X} r8={:08X} r9={:08X} r10={:08X} r11={:08X} r12={:08X} sp={:08X} lr={:08X} next_pc={:08X} decoded={:?}",
        fetched.pc,
        fetched.raw,
        fetched.instruction_set,
        step_cycles,
        emulator.cpu_cycles(),
        emulator.cpu_cpsr(),
        emulator.cpu_mode(),
        ie,
        if_reg,
        waitcnt,
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

#[cfg(test)]
mod tests {
    use super::gba_executable_region;

    #[test]
    fn gba_executable_region_allows_valid_code_areas() {
        assert!(gba_executable_region(0x0000_0000));
        assert!(gba_executable_region(0x0200_0000));
        assert!(gba_executable_region(0x0300_0000));
        assert!(gba_executable_region(0x0600_0000));
        assert!(gba_executable_region(0x0800_0000));
        assert!(gba_executable_region(0x0DFF_FFFE));
    }

    #[test]
    fn gba_executable_region_rejects_backup_and_open_bus_areas() {
        assert!(!gba_executable_region(0x0400_0000));
        assert!(!gba_executable_region(0x0E00_0000));
        assert!(!gba_executable_region(0x8328_9B90));
    }
}
