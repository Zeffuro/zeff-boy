use crate::debug::{CpuDebugSnapshot, DebugSection};
use zeff_md_core::Emulator;

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
        sections: vec![DebugSection {
            heading: "Status register",
            lines: vec![format!(
                "Trace:{}  Supervisor:{}  Interrupt mask:{}",
                u8::from(sr & 0x8000 != 0),
                u8::from(registers.supervisor()),
                (sr >> 8) & 7
            )],
        }],
        io_registers: Vec::new(),
        recent_opcodes: Vec::new(),
        call_stack: Vec::new(),
        call_stack_available: false,
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

#[cfg(test)]
mod tests;
