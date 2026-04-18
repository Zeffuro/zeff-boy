use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
#[cfg(test)]
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::{CpuState, ImeState};

// 0x00: NOP - No Operation
pub fn nop(_: &mut Cpu, _: &mut Bus) {}

// 0x10: STOP - Stop Cpu
pub fn stop(cpu: &mut Cpu, bus: &mut Bus) {
    let _ = cpu.fetch8_timed(bus);
    if bus.maybe_switch_cgb_speed() {
        let delay = 8200;

        let is_double_speed = bus.hardware_mode == HardwareMode::CGBDouble;
        let apu_delay = if is_double_speed { delay / 2 } else { delay };

        bus.step_apu(apu_delay);

        let prev_mode = bus.ppu_mode();

        let (int, new_mode) = bus.step_ppu(apu_delay);
        bus.if_reg |= int;
        bus.maybe_step_hblank_hdma(prev_mode, new_mode);

        let is_double_speed = bus.hardware_mode == HardwareMode::CGBDouble;
        cpu.timed_cycles_accounted += if is_double_speed { delay * 2 } else { delay };
        return;
    }
    cpu.running = CpuState::Stopped;
}

// 0x18: JR r8 - Relative Jump
pub fn jr_r8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus) as i8;
    cpu.jump_relative(offset);
    cpu.tick_internal_timed(bus, 4);
}

// 0x20: JR NZ, r8 - Relative Jump if Z
pub fn jr_nz_r8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus) as i8;
    if !cpu.get_z() {
        cpu.jump_relative(offset);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0x28: JR Z r8 - Relative Jump if Z
pub fn jr_z_r8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus) as i8;
    if cpu.get_z() {
        cpu.jump_relative(offset);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0x30: JR NC, r8 - Relative Jump if not Carry
pub fn jr_nc_r8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus) as i8;
    if !cpu.get_c() {
        cpu.jump_relative(offset);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0x38: JR C, r8 - Relative Jump if Carry
pub fn jr_c_r8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus) as i8;
    if cpu.get_c() {
        cpu.jump_relative(offset);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0x76: HALT - Halt Cpu until interrupt
pub fn halt(cpu: &mut Cpu, bus: &mut Bus) {
    let pending = bus.if_reg & bus.ie & 0x1F;
    if matches!(cpu.ime, ImeState::Disabled | ImeState::PendingEnable) && pending != 0 {
        cpu.trigger_halt_bug();
        return;
    }

    cpu.running = CpuState::Halted;
}

// 0xC0: RET NZ - Return if not Zero
pub fn ret_nz(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    if !cpu.get_z() {
        let addr = cpu.pop16_timed(bus);
        cpu.tick_internal_timed(bus, 4);
        cpu.jump(addr);
    }
}

// 0xC2: JP NZ, a16 - Jump if not Zero
pub fn jp_nz_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if !cpu.get_z() {
        cpu.jump(addr);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0xC3: JP a16 - Jump to address
pub fn jp_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.jump(addr);
    cpu.tick_internal_timed(bus, 4);
}

// 0xC4: CALL NZ, (a16) - Call if not Z
pub fn call_nz_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if !cpu.get_z() {
        cpu.tick_internal_timed(bus, 4);
        cpu.push16_timed(bus, cpu.pc);
        cpu.jump(addr);
    }
}

// 0xC7: RST 00H
pub fn rst_00(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0000;
}

// 0xC8: RET Z - Return if Zero
pub fn ret_z(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    if cpu.get_z() {
        let addr = cpu.pop16_timed(bus);
        cpu.tick_internal_timed(bus, 4);
        cpu.jump(addr);
    }
}

// 0xC9: RET - Return from subroutine
pub fn ret(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let addr = cpu.pop16_timed(bus);
    cpu.jump(addr);
}

// 0xCA: JP Z, a16 - Jump if Zero
pub fn jp_z_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if cpu.get_z() {
        cpu.jump(addr);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0xCC: CALL Z, a16 - Call if Zero
pub fn call_z_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if cpu.get_z() {
        cpu.tick_internal_timed(bus, 4);
        cpu.push16_timed(bus, cpu.pc);
        cpu.jump(addr);
    }
}

// 0xCD: Call a16 - Call subroutine at address
pub fn call_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.jump(addr);
}

// 0xCF: RST 08H
pub fn rst_08(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0008;
}

// 0xD0: RET NC - Return if not Carry
pub fn ret_nc(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    if !cpu.get_c() {
        let addr = cpu.pop16_timed(bus);
        cpu.tick_internal_timed(bus, 4);
        cpu.jump(addr);
    }
}

// 0xD2: JP NC, a16 - Jump if not Carry
pub fn jp_nc_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if !cpu.get_c() {
        cpu.jump(addr);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0xD4: CALL NC, a16 - Call if not Carry
pub fn call_nc_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if !cpu.get_c() {
        cpu.tick_internal_timed(bus, 4);
        cpu.push16_timed(bus, cpu.pc);
        cpu.jump(addr);
    }
}

// 0xD7: RST 10H
pub fn rst_10(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0010;
}

// 0xD8: RET C - Return if Carry
pub fn ret_c(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    if cpu.get_c() {
        let addr = cpu.pop16_timed(bus);
        cpu.tick_internal_timed(bus, 4);
        cpu.jump(addr);
    }
}

// 0xD9: RETI - Return and enable interrupts
pub fn reti(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let addr = cpu.pop16_timed(bus);
    cpu.jump(addr);
    cpu.ime = ImeState::Enabled;
}

// 0xDA: JP C, a16 - Jump if Carry
pub fn jp_c_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if cpu.get_c() {
        cpu.jump(addr);
        cpu.tick_internal_timed(bus, 4);
    }
}

// 0xDC: CALL C, a16 - Call if Carry
pub fn call_c_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    if cpu.get_c() {
        cpu.tick_internal_timed(bus, 4);
        cpu.push16_timed(bus, cpu.pc);
        cpu.jump(addr);
    }
}

// 0xDF: RST 18H
pub fn rst_18(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0018;
}

// 0xE7: RST 20H
pub fn rst_20(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0020;
}

// 0xE9: JP HL - Jump to address in HL
pub fn jp_hl(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.pc = cpu.get_hl();
}

// 0xEF: RST 28H
pub fn rst_28(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0028;
}

// 0xF3: DI - Disable Interrupts
pub fn di(cpu: &mut Cpu, _: &mut Bus) {
    cpu.ime = ImeState::Disabled;
}

// 0xF7: RST 30H
pub fn rst_30(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0030;
}

// 0xFB: EI - Enable interrupts (delayed by one instruction)
pub fn ei(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.ime = ImeState::PendingEnable;
}

// 0xFF: RST 38H
pub fn rst_38(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.pc);
    cpu.pc = 0x0038;
}

#[cfg(test)]
mod tests;
