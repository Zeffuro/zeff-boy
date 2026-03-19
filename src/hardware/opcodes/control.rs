use crate::hardware::cpu::CPU;
use log::debug;

// 0x00: NOP - No Operation
pub fn nop(cpu: &mut CPU) {
    debug!("NOP executed at PC={:04X}", cpu.pc);
    cpu.pc = cpu.pc.wrapping_add(1);
}

// 0xC3: 