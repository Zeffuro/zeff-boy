use super::*;

pub(super) fn snapshot(emu: &Emulator) -> CpuDebugSnapshot {
    let cpu = emu.sound_cpu();
    let registers = cpu.regs();
    let mut snapshot = super::snapshot(emu);
    let section = super::io::sound_section(emu);
    snapshot.register_lines = section.lines.clone();
    snapshot.sections = vec![section];
    snapshot.flags = [
        ('S', 0x80),
        ('Z', 0x40),
        ('H', 0x10),
        ('P', 4),
        ('N', 2),
        ('C', 1),
    ]
    .map(|(name, mask)| (name, registers.f & mask != 0))
    .to_vec();
    snapshot.status_text = "SOUND Z80 published register snapshot".into();
    snapshot.cpu_state = "Read-only; SOUND execution controls unavailable".into();
    snapshot.pc = registers.pc.into();
    snapshot.cycles = cpu.cycles();
    snapshot.io_registers.clear();
    snapshot.call_stack.clear();
    snapshot.call_stack_available = false;
    snapshot
}
