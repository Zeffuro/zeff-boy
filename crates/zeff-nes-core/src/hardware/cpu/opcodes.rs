use super::registers::*;
use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

/// Execute a single opcode and return the number of CPU cycles consumed.
///
/// This is a large match dispatch — fill in individual arms as you implement
/// each instruction. Every unimplemented opcode currently logs a warning and
/// returns 2 cycles.
pub(crate) fn execute(cpu: &mut Cpu, bus: &mut Bus, opcode: u8) -> u64 {
    match opcode {

        // LDA
        0xA9 => { let a = cpu.addr_immediate(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); 2 }
        0xA5 => { let a = cpu.addr_zero_page(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); 3 }
        0xB5 => { let a = cpu.addr_zero_page_x(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); 4 }
        0xAD => { let a = cpu.addr_absolute(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); 4 }
        0xBD => { let (a, c) = cpu.addr_absolute_x(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); if c { 5 } else { 4 } }
        0xB9 => { let (a, c) = cpu.addr_absolute_y(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); if c { 5 } else { 4 } }
        0xA1 => { let a = cpu.addr_indirect_x(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); 6 }
        0xB1 => { let (a, c) = cpu.addr_indirect_y(bus); let v = bus.cpu_read(a); cpu.regs.a = v; cpu.regs.set_zn(v); if c { 6 } else { 5 } }

        // LDX
        0xA2 => { let a = cpu.addr_immediate(bus); let v = bus.cpu_read(a); cpu.regs.x = v; cpu.regs.set_zn(v); 2 }
        0xA6 => { let a = cpu.addr_zero_page(bus); let v = bus.cpu_read(a); cpu.regs.x = v; cpu.regs.set_zn(v); 3 }
        0xB6 => { let a = cpu.addr_zero_page_y(bus); let v = bus.cpu_read(a); cpu.regs.x = v; cpu.regs.set_zn(v); 4 }
        0xAE => { let a = cpu.addr_absolute(bus); let v = bus.cpu_read(a); cpu.regs.x = v; cpu.regs.set_zn(v); 4 }
        0xBE => { let (a, c) = cpu.addr_absolute_y(bus); let v = bus.cpu_read(a); cpu.regs.x = v; cpu.regs.set_zn(v); if c { 5 } else { 4 } }

        // LDY
        0xA0 => { let a = cpu.addr_immediate(bus); let v = bus.cpu_read(a); cpu.regs.y = v; cpu.regs.set_zn(v); 2 }
        0xA4 => { let a = cpu.addr_zero_page(bus); let v = bus.cpu_read(a); cpu.regs.y = v; cpu.regs.set_zn(v); 3 }
        0xB4 => { let a = cpu.addr_zero_page_x(bus); let v = bus.cpu_read(a); cpu.regs.y = v; cpu.regs.set_zn(v); 4 }
        0xAC => { let a = cpu.addr_absolute(bus); let v = bus.cpu_read(a); cpu.regs.y = v; cpu.regs.set_zn(v); 4 }
        0xBC => { let (a, c) = cpu.addr_absolute_x(bus); let v = bus.cpu_read(a); cpu.regs.y = v; cpu.regs.set_zn(v); if c { 5 } else { 4 } }

        // STA
        0x85 => { let a = cpu.addr_zero_page(bus); bus.cpu_write(a, cpu.regs.a); 3 }
        0x95 => { let a = cpu.addr_zero_page_x(bus); bus.cpu_write(a, cpu.regs.a); 4 }
        0x8D => { let a = cpu.addr_absolute(bus); bus.cpu_write(a, cpu.regs.a); 4 }
        0x9D => { let (a, _) = cpu.addr_absolute_x(bus); bus.cpu_write(a, cpu.regs.a); 5 }
        0x99 => { let (a, _) = cpu.addr_absolute_y(bus); bus.cpu_write(a, cpu.regs.a); 5 }
        0x81 => { let a = cpu.addr_indirect_x(bus); bus.cpu_write(a, cpu.regs.a); 6 }
        0x91 => { let (a, _) = cpu.addr_indirect_y(bus); bus.cpu_write(a, cpu.regs.a); 6 }

        // STX
        0x86 => { let a = cpu.addr_zero_page(bus); bus.cpu_write(a, cpu.regs.x); 3 }
        0x96 => { let a = cpu.addr_zero_page_y(bus); bus.cpu_write(a, cpu.regs.x); 4 }
        0x8E => { let a = cpu.addr_absolute(bus); bus.cpu_write(a, cpu.regs.x); 4 }

        // STY
        0x84 => { let a = cpu.addr_zero_page(bus); bus.cpu_write(a, cpu.regs.y); 3 }
        0x94 => { let a = cpu.addr_zero_page_x(bus); bus.cpu_write(a, cpu.regs.y); 4 }
        0x8C => { let a = cpu.addr_absolute(bus); bus.cpu_write(a, cpu.regs.y); 4 }

        0xAA => { cpu.regs.x = cpu.regs.a; cpu.regs.set_zn(cpu.regs.x); 2 } // TAX
        0xA8 => { cpu.regs.y = cpu.regs.a; cpu.regs.set_zn(cpu.regs.y); 2 } // TAY
        0xBA => { cpu.regs.x = cpu.sp;     cpu.regs.set_zn(cpu.regs.x); 2 } // TSX
        0x8A => { cpu.regs.a = cpu.regs.x; cpu.regs.set_zn(cpu.regs.a); 2 } // TXA
        0x9A => { cpu.sp = cpu.regs.x;                                   2 } // TXS
        0x98 => { cpu.regs.a = cpu.regs.y; cpu.regs.set_zn(cpu.regs.a); 2 } // TYA

        0x48 => { cpu.push8(bus, cpu.regs.a); 3 }                              // PHA
        0x08 => { cpu.push8(bus, cpu.regs.status_for_push(true)); 3 }           // PHP
        0x68 => { let v = cpu.pop8(bus); cpu.regs.a = v; cpu.regs.set_zn(v); 4 } // PLA
        0x28 => { let v = cpu.pop8(bus); cpu.regs.p = (v & 0xEF) | 0x20; 4 }   // PLP

        // ADC
        0x69 => { let a = cpu.addr_immediate(bus); adc(cpu, bus.cpu_read(a)); 2 }
        0x65 => { let a = cpu.addr_zero_page(bus); adc(cpu, bus.cpu_read(a)); 3 }
        0x75 => { let a = cpu.addr_zero_page_x(bus); adc(cpu, bus.cpu_read(a)); 4 }
        0x6D => { let a = cpu.addr_absolute(bus); adc(cpu, bus.cpu_read(a)); 4 }
        0x7D => { let (a, c) = cpu.addr_absolute_x(bus); adc(cpu, bus.cpu_read(a)); if c { 5 } else { 4 } }
        0x79 => { let (a, c) = cpu.addr_absolute_y(bus); adc(cpu, bus.cpu_read(a)); if c { 5 } else { 4 } }
        0x61 => { let a = cpu.addr_indirect_x(bus); adc(cpu, bus.cpu_read(a)); 6 }
        0x71 => { let (a, c) = cpu.addr_indirect_y(bus); adc(cpu, bus.cpu_read(a)); if c { 6 } else { 5 } }

        // SBC
        0xE9 => { let a = cpu.addr_immediate(bus); sbc(cpu, bus.cpu_read(a)); 2 }
        0xE5 => { let a = cpu.addr_zero_page(bus); sbc(cpu, bus.cpu_read(a)); 3 }
        0xF5 => { let a = cpu.addr_zero_page_x(bus); sbc(cpu, bus.cpu_read(a)); 4 }
        0xED => { let a = cpu.addr_absolute(bus); sbc(cpu, bus.cpu_read(a)); 4 }
        0xFD => { let (a, c) = cpu.addr_absolute_x(bus); sbc(cpu, bus.cpu_read(a)); if c { 5 } else { 4 } }
        0xF9 => { let (a, c) = cpu.addr_absolute_y(bus); sbc(cpu, bus.cpu_read(a)); if c { 5 } else { 4 } }
        0xE1 => { let a = cpu.addr_indirect_x(bus); sbc(cpu, bus.cpu_read(a)); 6 }
        0xF1 => { let (a, c) = cpu.addr_indirect_y(bus); sbc(cpu, bus.cpu_read(a)); if c { 6 } else { 5 } }

        // CMP
        0xC9 => { let a = cpu.addr_immediate(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); 2 }
        0xC5 => { let a = cpu.addr_zero_page(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); 3 }
        0xD5 => { let a = cpu.addr_zero_page_x(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); 4 }
        0xCD => { let a = cpu.addr_absolute(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); 4 }
        0xDD => { let (a, c) = cpu.addr_absolute_x(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); if c { 5 } else { 4 } }
        0xD9 => { let (a, c) = cpu.addr_absolute_y(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); if c { 5 } else { 4 } }
        0xC1 => { let a = cpu.addr_indirect_x(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); 6 }
        0xD1 => { let (a, c) = cpu.addr_indirect_y(bus); cmp(cpu, cpu.regs.a, bus.cpu_read(a)); if c { 6 } else { 5 } }

        // CPX
        0xE0 => { let a = cpu.addr_immediate(bus); cmp(cpu, cpu.regs.x, bus.cpu_read(a)); 2 }
        0xE4 => { let a = cpu.addr_zero_page(bus); cmp(cpu, cpu.regs.x, bus.cpu_read(a)); 3 }
        0xEC => { let a = cpu.addr_absolute(bus); cmp(cpu, cpu.regs.x, bus.cpu_read(a)); 4 }

        // CPY
        0xC0 => { let a = cpu.addr_immediate(bus); cmp(cpu, cpu.regs.y, bus.cpu_read(a)); 2 }
        0xC4 => { let a = cpu.addr_zero_page(bus); cmp(cpu, cpu.regs.y, bus.cpu_read(a)); 3 }
        0xCC => { let a = cpu.addr_absolute(bus); cmp(cpu, cpu.regs.y, bus.cpu_read(a)); 4 }

        // INC
        0xE6 => { let a = cpu.addr_zero_page(bus); inc_mem(cpu, bus, a); 5 }
        0xF6 => { let a = cpu.addr_zero_page_x(bus); inc_mem(cpu, bus, a); 6 }
        0xEE => { let a = cpu.addr_absolute(bus); inc_mem(cpu, bus, a); 6 }
        0xFE => { let (a, _) = cpu.addr_absolute_x(bus); inc_mem(cpu, bus, a); 7 }

        // DEC
        0xC6 => { let a = cpu.addr_zero_page(bus); dec_mem(cpu, bus, a); 5 }
        0xD6 => { let a = cpu.addr_zero_page_x(bus); dec_mem(cpu, bus, a); 6 }
        0xCE => { let a = cpu.addr_absolute(bus); dec_mem(cpu, bus, a); 6 }
        0xDE => { let (a, _) = cpu.addr_absolute_x(bus); dec_mem(cpu, bus, a); 7 }

        0xE8 => { cpu.regs.x = cpu.regs.x.wrapping_add(1); cpu.regs.set_zn(cpu.regs.x); 2 } // INX
        0xC8 => { cpu.regs.y = cpu.regs.y.wrapping_add(1); cpu.regs.set_zn(cpu.regs.y); 2 } // INY
        0xCA => { cpu.regs.x = cpu.regs.x.wrapping_sub(1); cpu.regs.set_zn(cpu.regs.x); 2 } // DEX
        0x88 => { cpu.regs.y = cpu.regs.y.wrapping_sub(1); cpu.regs.set_zn(cpu.regs.y); 2 } // DEY

        // AND
        0x29 => { let a = cpu.addr_immediate(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 2 }
        0x25 => { let a = cpu.addr_zero_page(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 3 }
        0x35 => { let a = cpu.addr_zero_page_x(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 4 }
        0x2D => { let a = cpu.addr_absolute(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 4 }
        0x3D => { let (a, c) = cpu.addr_absolute_x(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 5 } else { 4 } }
        0x39 => { let (a, c) = cpu.addr_absolute_y(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 5 } else { 4 } }
        0x21 => { let a = cpu.addr_indirect_x(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 6 }
        0x31 => { let (a, c) = cpu.addr_indirect_y(bus); cpu.regs.a &= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 6 } else { 5 } }

        // ORA
        0x09 => { let a = cpu.addr_immediate(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 2 }
        0x05 => { let a = cpu.addr_zero_page(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 3 }
        0x15 => { let a = cpu.addr_zero_page_x(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 4 }
        0x0D => { let a = cpu.addr_absolute(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 4 }
        0x1D => { let (a, c) = cpu.addr_absolute_x(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 5 } else { 4 } }
        0x19 => { let (a, c) = cpu.addr_absolute_y(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 5 } else { 4 } }
        0x01 => { let a = cpu.addr_indirect_x(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 6 }
        0x11 => { let (a, c) = cpu.addr_indirect_y(bus); cpu.regs.a |= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 6 } else { 5 } }

        // EOR
        0x49 => { let a = cpu.addr_immediate(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 2 }
        0x45 => { let a = cpu.addr_zero_page(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 3 }
        0x55 => { let a = cpu.addr_zero_page_x(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 4 }
        0x4D => { let a = cpu.addr_absolute(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 4 }
        0x5D => { let (a, c) = cpu.addr_absolute_x(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 5 } else { 4 } }
        0x59 => { let (a, c) = cpu.addr_absolute_y(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 5 } else { 4 } }
        0x41 => { let a = cpu.addr_indirect_x(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); 6 }
        0x51 => { let (a, c) = cpu.addr_indirect_y(bus); cpu.regs.a ^= bus.cpu_read(a); cpu.regs.set_zn(cpu.regs.a); if c { 6 } else { 5 } }

        // BIT
        0x24 => { let a = cpu.addr_zero_page(bus); bit(cpu, bus.cpu_read(a)); 3 }
        0x2C => { let a = cpu.addr_absolute(bus); bit(cpu, bus.cpu_read(a)); 4 }

        // ASL
        0x0A => { asl_acc(cpu); 2 }
        0x06 => { let a = cpu.addr_zero_page(bus); asl_mem(cpu, bus, a); 5 }
        0x16 => { let a = cpu.addr_zero_page_x(bus); asl_mem(cpu, bus, a); 6 }
        0x0E => { let a = cpu.addr_absolute(bus); asl_mem(cpu, bus, a); 6 }
        0x1E => { let (a, _) = cpu.addr_absolute_x(bus); asl_mem(cpu, bus, a); 7 }

        // LSR
        0x4A => { lsr_acc(cpu); 2 }
        0x46 => { let a = cpu.addr_zero_page(bus); lsr_mem(cpu, bus, a); 5 }
        0x56 => { let a = cpu.addr_zero_page_x(bus); lsr_mem(cpu, bus, a); 6 }
        0x4E => { let a = cpu.addr_absolute(bus); lsr_mem(cpu, bus, a); 6 }
        0x5E => { let (a, _) = cpu.addr_absolute_x(bus); lsr_mem(cpu, bus, a); 7 }

        // ROL
        0x2A => { rol_acc(cpu); 2 }
        0x26 => { let a = cpu.addr_zero_page(bus); rol_mem(cpu, bus, a); 5 }
        0x36 => { let a = cpu.addr_zero_page_x(bus); rol_mem(cpu, bus, a); 6 }
        0x2E => { let a = cpu.addr_absolute(bus); rol_mem(cpu, bus, a); 6 }
        0x3E => { let (a, _) = cpu.addr_absolute_x(bus); rol_mem(cpu, bus, a); 7 }

        // ROR
        0x6A => { ror_acc(cpu); 2 }
        0x66 => { let a = cpu.addr_zero_page(bus); ror_mem(cpu, bus, a); 5 }
        0x76 => { let a = cpu.addr_zero_page_x(bus); ror_mem(cpu, bus, a); 6 }
        0x6E => { let a = cpu.addr_absolute(bus); ror_mem(cpu, bus, a); 6 }
        0x7E => { let (a, _) = cpu.addr_absolute_x(bus); ror_mem(cpu, bus, a); 7 }

        0x90 => branch(cpu, bus, !cpu.regs.get_flag(CARRY_FLAG)),    // BCC
        0xB0 => branch(cpu, bus,  cpu.regs.get_flag(CARRY_FLAG)),    // BCS
        0xF0 => branch(cpu, bus,  cpu.regs.get_flag(ZERO_FLAG)),     // BEQ
        0xD0 => branch(cpu, bus, !cpu.regs.get_flag(ZERO_FLAG)),     // BNE
        0x30 => branch(cpu, bus,  cpu.regs.get_flag(NEGATIVE_FLAG)), // BMI
        0x10 => branch(cpu, bus, !cpu.regs.get_flag(NEGATIVE_FLAG)), // BPL
        0x70 => branch(cpu, bus,  cpu.regs.get_flag(OVERFLOW_FLAG)), // BVS
        0x50 => branch(cpu, bus, !cpu.regs.get_flag(OVERFLOW_FLAG)), // BVC

        0x4C => { let a = cpu.addr_absolute(bus); cpu.pc = a; 3 }           // JMP abs
        0x6C => { let a = cpu.addr_indirect(bus); cpu.pc = a; 5 }           // JMP (ind)
        0x20 => { let a = cpu.addr_absolute(bus); cpu.push16(bus, cpu.pc.wrapping_sub(1)); cpu.pc = a; 6 } // JSR
        0x60 => { let a = cpu.pop16(bus); cpu.pc = a.wrapping_add(1); 6 }   // RTS
        0x40 => {                                                            // RTI
            let p = cpu.pop8(bus);
            cpu.regs.p = (p & 0xEF) | 0x20;
            cpu.pc = cpu.pop16(bus);
            6
        }

        0x18 => { cpu.regs.set_flag(CARRY_FLAG, false);     2 } // CLC
        0x38 => { cpu.regs.set_flag(CARRY_FLAG, true);      2 } // SEC
        0x58 => { cpu.regs.set_flag(INTERRUPT_FLAG, false);  2 } // CLI
        0x78 => { cpu.regs.set_flag(INTERRUPT_FLAG, true);   2 } // SEI
        0xD8 => { cpu.regs.set_flag(DECIMAL_FLAG, false);    2 } // CLD
        0xF8 => { cpu.regs.set_flag(DECIMAL_FLAG, true);     2 } // SED
        0xB8 => { cpu.regs.set_flag(OVERFLOW_FLAG, false);   2 } // CLV

        0x00 => { // BRK
            cpu.pc = cpu.pc.wrapping_add(1);
            cpu.push16(bus, cpu.pc);
            cpu.push8(bus, cpu.regs.status_for_push(true));
            cpu.regs.set_flag(INTERRUPT_FLAG, true);
            let lo = bus.cpu_read(0xFFFE) as u16;
            let hi = bus.cpu_read(0xFFFF) as u16;
            cpu.pc = (hi << 8) | lo;
            7
        }
        0xEA => 2, // NOP

        _ => {
            log::warn!(
                "Unimplemented opcode {:#04X} at PC={:#06X}",
                opcode,
                cpu.pc.wrapping_sub(1)
            );
            2
        }
    }
}

fn adc(cpu: &mut Cpu, val: u8) {
    let a = cpu.regs.a as u16;
    let v = val as u16;
    let c = if cpu.regs.get_flag(CARRY_FLAG) { 1u16 } else { 0 };
    let sum = a + v + c;
    let result = sum as u8;
    cpu.regs.set_flag(CARRY_FLAG, sum > 0xFF);
    cpu.regs.set_flag(OVERFLOW_FLAG, (!(a ^ v) & (a ^ sum)) & 0x80 != 0);
    cpu.regs.a = result;
    cpu.regs.set_zn(result);
}

fn sbc(cpu: &mut Cpu, val: u8) {
    adc(cpu, !val);
}

fn cmp(cpu: &mut Cpu, reg: u8, val: u8) {
    let diff = reg.wrapping_sub(val);
    cpu.regs.set_flag(CARRY_FLAG, reg >= val);
    cpu.regs.set_zn(diff);
}

fn bit(cpu: &mut Cpu, val: u8) {
    cpu.regs.set_flag(ZERO_FLAG, cpu.regs.a & val == 0);
    cpu.regs.set_flag(OVERFLOW_FLAG, val & 0x40 != 0);
    cpu.regs.set_flag(NEGATIVE_FLAG, val & 0x80 != 0);
}

fn inc_mem(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let v = bus.cpu_read(addr).wrapping_add(1);
    bus.cpu_write(addr, v);
    cpu.regs.set_zn(v);
}

fn dec_mem(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let v = bus.cpu_read(addr).wrapping_sub(1);
    bus.cpu_write(addr, v);
    cpu.regs.set_zn(v);
}

fn asl_acc(cpu: &mut Cpu) {
    let old = cpu.regs.a;
    cpu.regs.a = old << 1;
    cpu.regs.set_flag(CARRY_FLAG, old & 0x80 != 0);
    cpu.regs.set_zn(cpu.regs.a);
}

fn asl_mem(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let old = bus.cpu_read(addr);
    let v = old << 1;
    bus.cpu_write(addr, v);
    cpu.regs.set_flag(CARRY_FLAG, old & 0x80 != 0);
    cpu.regs.set_zn(v);
}

fn lsr_acc(cpu: &mut Cpu) {
    let old = cpu.regs.a;
    cpu.regs.a = old >> 1;
    cpu.regs.set_flag(CARRY_FLAG, old & 0x01 != 0);
    cpu.regs.set_zn(cpu.regs.a);
}

fn lsr_mem(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let old = bus.cpu_read(addr);
    let v = old >> 1;
    bus.cpu_write(addr, v);
    cpu.regs.set_flag(CARRY_FLAG, old & 0x01 != 0);
    cpu.regs.set_zn(v);
}

fn rol_acc(cpu: &mut Cpu) {
    let old = cpu.regs.a;
    let carry_in = if cpu.regs.get_flag(CARRY_FLAG) { 1 } else { 0 };
    cpu.regs.a = (old << 1) | carry_in;
    cpu.regs.set_flag(CARRY_FLAG, old & 0x80 != 0);
    cpu.regs.set_zn(cpu.regs.a);
}

fn rol_mem(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let old = bus.cpu_read(addr);
    let carry_in = if cpu.regs.get_flag(CARRY_FLAG) { 1 } else { 0 };
    let v = (old << 1) | carry_in;
    bus.cpu_write(addr, v);
    cpu.regs.set_flag(CARRY_FLAG, old & 0x80 != 0);
    cpu.regs.set_zn(v);
}

fn ror_acc(cpu: &mut Cpu) {
    let old = cpu.regs.a;
    let carry_in = if cpu.regs.get_flag(CARRY_FLAG) { 0x80 } else { 0 };
    cpu.regs.a = (old >> 1) | carry_in;
    cpu.regs.set_flag(CARRY_FLAG, old & 0x01 != 0);
    cpu.regs.set_zn(cpu.regs.a);
}

fn ror_mem(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let old = bus.cpu_read(addr);
    let carry_in = if cpu.regs.get_flag(CARRY_FLAG) { 0x80 } else { 0 };
    let v = (old >> 1) | carry_in;
    bus.cpu_write(addr, v);
    cpu.regs.set_flag(CARRY_FLAG, old & 0x01 != 0);
    cpu.regs.set_zn(v);
}

fn branch(cpu: &mut Cpu, bus: &mut Bus, condition: bool) -> u64 {
    let target = cpu.addr_relative(bus);
    if condition {
        let page_cross = (cpu.pc & 0xFF00) != (target & 0xFF00);
        cpu.pc = target;
        if page_cross { 4 } else { 3 }
    } else {
        2
    }
}

