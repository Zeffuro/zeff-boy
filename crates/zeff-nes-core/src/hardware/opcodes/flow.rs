use crate::hardware::constants::{IRQ_VECTOR_HI, IRQ_VECTOR_LO, NMI_VECTOR_HI, NMI_VECTOR_LO};
use crate::hardware::cpu::registers::StatusFlags;
use crate::hardware::cpu::{Cpu, CpuBus};

// 0x00: BRK
pub fn brk<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let _ = cpu.fetch8(bus);
    cpu.push16(bus, cpu.pc);
    let vector_edge = bus.take_nmi_edge_for_vector();
    let nmi_hijacked = cpu.nmi_pending || vector_edge;
    cpu.push8(bus, cpu.regs.status_for_push(true));
    cpu.regs.set_flag(StatusFlags::INTERRUPT, true);
    cpu.clear_irq_inhibit_delay();

    let (vec_lo, vec_hi) = if nmi_hijacked {
        cpu.nmi_pending = false;
        cpu.nmi_count = cpu.nmi_count.wrapping_add(1);
        (NMI_VECTOR_LO, NMI_VECTOR_HI)
    } else {
        (IRQ_VECTOR_LO, IRQ_VECTOR_HI)
    };
    let lo = bus.cpu_read(vec_lo) as u16;
    let hi = bus.cpu_read(vec_hi) as u16;
    cpu.pc = (hi << 8) | lo;
}

// 0xEA: NOP
pub fn nop<B: CpuBus>(_cpu: &mut Cpu, _bus: &mut B) {}

// Unofficial 2-byte NOPs (e.g. 0x80/0x82/0x89/0xC2/0xE2).
pub fn nop_imm<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let _ = cpu.fetch8(bus);
}

// Returns extra cycles: 0 (not taken), 1 (taken same page), 2 (taken + page cross)
fn branch<B: CpuBus>(cpu: &mut Cpu, bus: &mut B, condition: bool) -> u8 {
    let target = cpu.addr_relative(bus);
    if condition {
        let page_cross = (cpu.pc & 0xFF00) != (target & 0xFF00);
        let _ = bus.cpu_read(cpu.pc);
        if page_cross {
            let _ = bus.cpu_read((cpu.pc & 0xFF00) | (target & 0x00FF));
        }
        cpu.pc = target;
        if page_cross {
            2
        } else {
            cpu.mark_branch_taken_same_page();
            1
        }
    } else {
        0
    }
}

// 0x90: BCC
pub fn bcc<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(StatusFlags::CARRY))
}

// 0xB0: BCS
pub fn bcs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(StatusFlags::CARRY))
}

// 0xF0: BEQ
pub fn beq<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(StatusFlags::ZERO))
}

// 0xD0: BNE
pub fn bne<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(StatusFlags::ZERO))
}

// 0x30: BMI
pub fn bmi<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(StatusFlags::NEGATIVE))
}

// 0x10: BPL
pub fn bpl<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(StatusFlags::NEGATIVE))
}

// 0x70: BVS
pub fn bvs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(StatusFlags::OVERFLOW))
}

// 0x50: BVC
pub fn bvc<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(StatusFlags::OVERFLOW))
}

// 0x4C: JMP abs
pub fn jmp_abs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let a = cpu.addr_absolute(bus);
    cpu.pc = a;
}

// 0x6C: JMP (ind)
pub fn jmp_ind<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let a = cpu.addr_indirect(bus);
    cpu.pc = a;
}

// 0x20: JSR abs
pub fn jsr<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let lo = cpu.fetch8(bus) as u16;
    let _ = bus.cpu_read(crate::hardware::constants::STACK_BASE | u16::from(cpu.sp));
    cpu.push16(bus, cpu.pc);
    let hi = cpu.fetch8(bus) as u16;
    cpu.pc = (hi << 8) | lo;
}

// 0x60: RTS
pub fn rts<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let _ = bus.cpu_read(cpu.pc);
    let _ = bus.cpu_read(crate::hardware::constants::STACK_BASE | u16::from(cpu.sp));
    let a = cpu.pop16(bus);
    let _ = bus.cpu_read(a);
    cpu.pc = a.wrapping_add(1);
}

// 0x40: RTI
pub fn rti<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let _ = bus.cpu_read(cpu.pc);
    let _ = bus.cpu_read(crate::hardware::constants::STACK_BASE | u16::from(cpu.sp));
    let p = cpu.pop8(bus);
    cpu.regs.p = StatusFlags::from_bits_truncate((p & 0xEF) | 0x20);
    cpu.pc = cpu.pop16(bus);
    cpu.clear_irq_inhibit_delay();
}

// 0x18: CLC
pub fn clc<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    cpu.regs.set_flag(StatusFlags::CARRY, false);
}

// 0x38: SEC
pub fn sec<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    cpu.regs.set_flag(StatusFlags::CARRY, true);
}

// 0x58: CLI
pub fn cli<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    cpu.delay_irq_inhibit_change();
    cpu.regs.set_flag(StatusFlags::INTERRUPT, false);
}

// 0x78: SEI
pub fn sei<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    cpu.delay_irq_inhibit_change();
    cpu.regs.set_flag(StatusFlags::INTERRUPT, true);
}

// 0xD8: CLD
pub fn cld<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    cpu.regs.set_flag(StatusFlags::DECIMAL, false);
}

// 0xF8: SED
pub fn sed<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    cpu.regs.set_flag(StatusFlags::DECIMAL, true);
}

// 0xB8: CLV
pub fn clv<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    cpu.regs.set_flag(StatusFlags::OVERFLOW, false);
}
