use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::opcodes::cycles::CB_CYCLE_TABLE;


pub fn execute_cb_prefix(cpu: &mut Cpu, bus: &mut Bus) {
    let opcode = cpu.fetch8_timed(bus);

    crate::hardware::opcodes::bitwise::execute_cb_op(cpu, bus, opcode);

    let expected_total = CB_CYCLE_TABLE[opcode as usize] as u64;
    if cpu.timed_cycles_accounted < expected_total {
        cpu.tick_internal_timed(bus, expected_total - cpu.timed_cycles_accounted);
    }
}
