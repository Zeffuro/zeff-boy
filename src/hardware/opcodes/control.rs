use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;
use crate::hardware::types::{CPUState, IMEState};

// 0x00: NOP - No Operation
pub(crate) fn nop(_: &mut CPU, _: &mut Bus) {
}

// 0x10: STOP - Stop CPU
pub(crate) fn stop(cpu: &mut CPU, bus: &mut Bus) {
    let _ = cpu.fetch8(bus);
    cpu.running = CPUState::Stopped;
}

// 0x18: JR r8 - Relative Jump
pub(crate) fn jr_r8(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8(bus) as i8;
    cpu.jump_relative(offset);
}

// 0x20: JR NZ, r8 - Relative Jump if Z
pub(crate) fn jr_nz_r8(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8(bus) as i8;
    if !cpu.get_z() {
        cpu.jump_relative(offset);
        cpu.last_step_cycles += 4;
    }
}

// 0x28: JR Z r8 - Relative Jump if Z
pub(crate) fn jr_z_r8(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8(bus) as i8;
    if cpu.get_z() {
        cpu.jump_relative(offset);
        cpu.last_step_cycles += 4;
    }
}

// 0x30: JR NC, r8 - Relative Jump if not Carry
pub(crate) fn jr_nc_r8(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8(bus) as i8;
    if !cpu.get_c() {
        cpu.jump_relative(offset);
        cpu.last_step_cycles += 4;
    }
}

// 0x38: JR C, r8 - Relative Jump if Carry
pub(crate) fn jr_c_r8(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8(bus) as i8;
    if cpu.get_c() {
        cpu.jump_relative(offset);
        cpu.last_step_cycles += 4;
    }
}

// 0x76: HALT - Halt CPU until interrupt
pub(crate) fn halt(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.running = CPUState::Halted;
}

// 0xC0: RET NZ - Return if not Zero
pub(crate) fn ret_nz(cpu: &mut CPU, bus: &mut Bus) {
    if !cpu.get_z() {
        let addr = cpu.pop16(bus);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xC2: JP NZ, a16 - Jump if not Zero
pub(crate) fn jp_nz_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if !cpu.get_z() {
        cpu.jump(addr);
        cpu.last_step_cycles += 4;
    }
}


// 0xC3: JP a16 - Jump to address
pub(crate) fn jp_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    cpu.jump(addr);
}

// 0xC4: CALL NZ, (a16) - Call if not Z
pub(crate) fn call_nz_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if !cpu.get_z() {
        cpu.push16(bus, cpu.pc);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xC7: RST 00H
pub(crate) fn rst_00(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0000;
}

// 0xC8: RET Z - Return if Zero
pub(crate) fn ret_z(cpu: &mut CPU, bus: &mut Bus) {
    if cpu.get_z() {
        let addr = cpu.pop16(bus);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xC9: RET - Return from subroutine
pub(crate) fn ret(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.pop16(bus);
    cpu.jump(addr);
}

// 0xCA: JP Z, a16 - Jump if Zero
pub(crate) fn jp_z_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if cpu.get_z() {
        cpu.jump(addr);
        cpu.last_step_cycles += 4;
    }
}

// 0xCC: CALL Z, a16 - Call if Zero
pub(crate) fn call_z_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if cpu.get_z() {
        cpu.push16(bus, cpu.pc);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xCD: Call a16 - Call subroutine at address
pub(crate) fn call_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    cpu.push16(bus, cpu.pc);
    cpu.jump(addr);
}

// 0xCF: RST 08H
pub(crate) fn rst_08(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0008;
}

// 0xD0: RET NC - Return if not Carry
pub(crate) fn ret_nc(cpu: &mut CPU, bus: &mut Bus) {
    if !cpu.get_c() {
        let addr = cpu.pop16(bus);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xD2: JP NC, a16 - Jump if not Carry
pub(crate) fn jp_nc_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if !cpu.get_c() {
        cpu.jump(addr);
        cpu.last_step_cycles += 4;
    }
}

// 0xD4: CALL NC, a16 - Call if not Carry
pub(crate) fn call_nc_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if !cpu.get_c() {
        cpu.push16(bus, cpu.pc);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xD7: RST 10H
pub(crate) fn rst_10(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0010;
}

// 0xD8: RET C - Return if Carry
pub(crate) fn ret_c(cpu: &mut CPU, bus: &mut Bus) {
    if cpu.get_c() {
        let addr = cpu.pop16(bus);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xD9: RETI - Return and enable interrupts
pub(crate) fn reti(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.pop16(bus);
    cpu.jump(addr);
    cpu.ime = IMEState::Enabled;
}

// 0xDA: JP C, a16 - Jump if Carry
pub(crate) fn jp_c_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if cpu.get_c() {
        cpu.jump(addr);
        cpu.last_step_cycles += 4;
    }
}

// 0xDC: CALL C, a16 - Call if Carry
pub(crate) fn call_c_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16(bus);
    if cpu.get_c() {
        cpu.push16(bus, cpu.pc);
        cpu.jump(addr);
        cpu.last_step_cycles += 12;
    }
}

// 0xDF: RST 18H
pub(crate) fn rst_18(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0018;
}

// 0xE7: RST 20H
pub(crate) fn rst_20(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0020;
}

// 0xE9: JP HL - Jump to address in HL
pub(crate) fn jp_hl(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.pc = cpu.get_hl();
}

// 0xEF: RST 28H
pub(crate) fn rst_28(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0028;
}

// 0xF3: DI - Disable Interrupts
pub(crate) fn di(cpu: &mut CPU, _: &mut Bus) {
    cpu.ime = IMEState::Disabled;
}

// 0xF7: RST 30H
pub(crate) fn rst_30(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0030;
}

// 0xFB: EI - Enable interrupts (delayed by one instruction)
pub(crate) fn ei(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.ime = IMEState::PendingEnable;
}

// 0xFF: RST 38H
pub(crate) fn rst_38(cpu: &mut CPU, bus: &mut Bus) {
    cpu.push16(bus, cpu.pc);
    cpu.pc = 0x0038;
}