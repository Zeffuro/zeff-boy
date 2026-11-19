use crate::debug::{CallStackDisplay, CpuDebugSnapshot, DebugSection};
use zeff_m68k::timing::ControlFlowEntryKind;
use zeff_md_core::Emulator;
use zeff_md_core::emulator::CallStackStatus;

mod io;
mod sound;

pub(super) fn snapshot_target(
    emu: &Emulator,
    target: crate::debug::DebugTarget,
) -> CpuDebugSnapshot {
    match target {
        crate::debug::DebugTarget::Primary => snapshot(emu),
        crate::debug::DebugTarget::Sound => sound::snapshot(emu),
    }
}

pub(super) fn snapshot(emu: &Emulator) -> CpuDebugSnapshot {
    let registers = emu.registers();
    let sr = registers.status_register();
    let mut register_lines = Vec::with_capacity(10);
    for pair in 0..4 {
        let index = pair * 2;
        register_lines.push(format!(
            "D{index}:{:08X}  D{}:{:08X}",
            registers.d[index],
            index + 1,
            registers.d[index + 1]
        ));
    }
    for pair in 0..4 {
        let index = pair * 2;
        register_lines.push(format!(
            "A{index}:{:08X}  A{}:{:08X}",
            registers.address_register(index).unwrap(),
            index + 1,
            registers.address_register(index + 1).unwrap()
        ));
    }
    register_lines.push(format!(
        "USP:{:08X}  SSP:{:08X}",
        registers.usp, registers.ssp
    ));
    register_lines.push(format!("PC:{:08X}  SR:{sr:04X}", registers.pc));

    CpuDebugSnapshot {
        register_lines,
        flags: [('X', 0x10), ('N', 8), ('Z', 4), ('V', 2), ('C', 1)]
            .map(|(name, mask)| (name, sr & mask != 0))
            .to_vec(),
        status_text: "M68000 register snapshot".into(),
        cpu_state: "Read-only".into(),
        pc: registers.pc,
        cycles: emu.cpu_clock().0,
        last_opcode_line: "Instruction history unavailable".into(),
        sections: sections(emu, sr, registers.supervisor()),
        io_registers: io::registers(emu),
        recent_opcodes: Vec::new(),
        call_stack: emu
            .call_stack()
            .map(|frame| CallStackDisplay {
                target: frame.target_pc,
                return_address: frame.return_pc,
                target_rom_offset: None,
                return_rom_offset: None,
                kind: match frame.kind {
                    ControlFlowEntryKind::Jsr => "JSR",
                    ControlFlowEntryKind::Bsr => "BSR",
                    ControlFlowEntryKind::Debugger => "DBG",
                    ControlFlowEntryKind::Interrupt { .. } => "INT",
                    ControlFlowEntryKind::Exception(9) => "TRACE",
                    ControlFlowEntryKind::Exception(32..=47) => "TRAP",
                    ControlFlowEntryKind::Exception(_) => "EXC",
                },
            })
            .collect(),
        call_stack_available: true,
        breakpoints: Vec::new(),
        one_shot_breakpoints: Vec::new(),
        breakpoint_hit_conditions: Vec::new(),
        supported_events: Vec::new(),
        event_breakpoints: Vec::new(),
        rom_breakpoints: Vec::new(),
        watchpoints: Vec::new(),
        hit_breakpoint: None,
        hit_rom_breakpoint: None,
        hit_watchpoint: None,
        hit_event: None,
    }
}

fn sections(emu: &Emulator, sr: u16, supervisor: bool) -> Vec<DebugSection> {
    let mut sections = vec![DebugSection {
        heading: "Status register",
        lines: vec![format!(
            "Trace:{}  Supervisor:{}  Interrupt mask:{}",
            u8::from(sr & 0x8000 != 0),
            u8::from(supervisor),
            (sr >> 8) & 7
        )],
    }];
    sections.extend(io::sections(emu));
    let status = match emu.call_stack_status() {
        CallStackStatus::Disabled => return sections,
        CallStackStatus::Unknown => "Earlier calls are unknown; recording new calls",
        CallStackStatus::Observed { truncated: true } => {
            "Observed call suffix; older captured frames were dropped"
        }
        CallStackStatus::Observed { truncated: false } => {
            "Observed call suffix; earlier calls may be absent"
        }
    };
    sections.push(DebugSection {
        heading: "Call stack",
        lines: vec![status.into()],
    });
    sections
}

#[cfg(test)]
mod tests;
