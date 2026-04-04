use zeff_gb_core::hardware::types::ImeState;
use zeff_gb_core::hardware::types::hardware_mode::HardwareMode;

use super::types::HeadlessOptions;

pub(super) fn ime_short(ime: &ImeState) -> &'static str {
    match ime {
        ImeState::Enabled => "E",
        ImeState::Disabled => "D",
        ImeState::PendingEnable => "P",
    }
}

pub(super) fn mode_short(mode: HardwareMode) -> &'static str {
    match mode {
        HardwareMode::DMG => "DMG",
        HardwareMode::SGB1 => "S1",
        HardwareMode::SGB2 => "S2",
        HardwareMode::CGBNormal => "CGB1",
        HardwareMode::CGBDouble => "CGB2",
    }
}

fn is_interrupt_watch_opcode(op: u8) -> bool {
    matches!(
        op,
        0x10 // STOP
            | 0x18 // JR n
            | 0x20 // JR NZ,n
            | 0x28 // JR Z,n
            | 0x30 // JR NC,n
            | 0x38 // JR C,n
            | 0x76 // HALT
            | 0xC2 // JP NZ,nn
            | 0xC3 // JP nn
            | 0xCA // JP Z,nn
            | 0xD2 // JP NC,nn
            | 0xD9 // RETI
            | 0xDA // JP C,nn
            | 0xE9 // JP HL
            | 0xF3 // DI
            | 0xFB // EI
    )
}

pub(super) struct CpuTraceState<'a> {
    pub pc: u16,
    pub op: u8,
    pub total_t: u64,
    pub ime: &'a ImeState,
    pub if_reg: u8,
    pub ie: u8,
}

pub(super) fn should_trace_op(opts: &HeadlessOptions, cpu: &CpuTraceState<'_>) -> bool {
    if cpu.total_t < opts.trace_start_t {
        return false;
    }

    if let Some((start, end)) = opts.trace_pc_range
        && (cpu.pc < start || cpu.pc > end)
    {
        return false;
    }

    if !opts.trace_opcode_filter.is_empty() && !opts.trace_opcode_filter.contains(&cpu.op) {
        return false;
    }

    if opts.trace_watch_interrupts {
        let pending = (cpu.if_reg & cpu.ie) & 0x1F;
        return is_interrupt_watch_opcode(cpu.op)
            || pending != 0
            || matches!(*cpu.ime, ImeState::PendingEnable);
    }

    true
}
