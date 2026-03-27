use crate::hardware::bus::Bus;
use crate::hardware::constants::{IRQ_VECTOR_LO, IRQ_VECTOR_HI, NMI_VECTOR_LO, NMI_VECTOR_HI};
use crate::hardware::cpu::Cpu;
use crate::hardware::cpu::registers::*;

// 0x00: BRK
pub fn brk(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.pc = cpu.pc.wrapping_add(1);
    cpu.push16(bus, cpu.pc);
    cpu.push8(bus, cpu.regs.status_for_push(true));
    cpu.regs.set_flag(INTERRUPT_FLAG, true);

    let (vec_lo, vec_hi) = if cpu.nmi_pending {
        cpu.nmi_pending = false;
        (NMI_VECTOR_LO, NMI_VECTOR_HI)
    } else {
        (IRQ_VECTOR_LO, IRQ_VECTOR_HI)
    };
    let lo = bus.cpu_read(vec_lo) as u16;
    let hi = bus.cpu_read(vec_hi) as u16;
    cpu.pc = (hi << 8) | lo;
}

// 0xEA: NOP
pub fn nop(_cpu: &mut Cpu, _bus: &mut Bus) {}

// Unofficial 2-byte NOPs (e.g. 0x80/0x82/0x89/0xC2/0xE2).
pub fn nop_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let _ = cpu.addr_immediate(bus);
}

// Returns extra cycles: 0 (not taken), 1 (taken same page), 2 (taken + page cross)
fn branch(cpu: &mut Cpu, bus: &mut Bus, condition: bool) -> u8 {
    let target = cpu.addr_relative(bus);
    if condition {
        let page_cross = (cpu.pc & 0xFF00) != (target & 0xFF00);
        cpu.pc = target;
        if page_cross { 2 } else { 1 }
    } else {
        0
    }
}

// 0x90: BCC
pub fn bcc(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(CARRY_FLAG))
}

// 0xB0: BCS
pub fn bcs(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(CARRY_FLAG))
}

// 0xF0: BEQ
pub fn beq(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(ZERO_FLAG))
}

// 0xD0: BNE
pub fn bne(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(ZERO_FLAG))
}

// 0x30: BMI
pub fn bmi(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(NEGATIVE_FLAG))
}

// 0x10: BPL
pub fn bpl(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(NEGATIVE_FLAG))
}

// 0x70: BVS
pub fn bvs(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(OVERFLOW_FLAG))
}

// 0x50: BVC
pub fn bvc(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(OVERFLOW_FLAG))
}

// 0x4C: JMP abs
pub fn jmp_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.pc = a;
}

// 0x6C: JMP (ind)
pub fn jmp_ind(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect(bus);
    cpu.pc = a;
}

// 0x20: JSR abs
pub fn jsr(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.push16(bus, cpu.pc.wrapping_sub(1));
    cpu.pc = a;
}

// 0x60: RTS
pub fn rts(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.pop16(bus);
    cpu.pc = a.wrapping_add(1);
}

// 0x40: RTI
pub fn rti(cpu: &mut Cpu, bus: &mut Bus) {
    let p = cpu.pop8(bus);
    cpu.regs.p = (p & 0xEF) | 0x20;
    cpu.pc = cpu.pop16(bus);
}

// 0x18: CLC
pub fn clc(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(CARRY_FLAG, false);
}

// 0x38: SEC
pub fn sec(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(CARRY_FLAG, true);
}

// 0x58: CLI
pub fn cli(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(INTERRUPT_FLAG, false);
}

// 0x78: SEI
pub fn sei(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(INTERRUPT_FLAG, true);
}

// 0xD8: CLD
pub fn cld(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(DECIMAL_FLAG, false);
}

// 0xF8: SED
pub fn sed(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(DECIMAL_FLAG, true);
}

// 0xB8: CLV
pub fn clv(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(OVERFLOW_FLAG, false);
}
