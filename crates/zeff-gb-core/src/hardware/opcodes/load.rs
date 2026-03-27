use crate::hardware::bus::{Bus, OamCorruptionType};
use crate::hardware::cpu::Cpu;
use crate::hardware::types::constants as memory_constants;

// 0x01 - LD BC, (d16)
pub fn ld_bc_d16(cpu: &mut Cpu, bus: &mut Bus) {
    let value = cpu.fetch16_timed(bus);
    cpu.set_bc(value);
}

// 0x02 - LD (BC), A
pub fn ld_bc_a(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_bc(), cpu.regs.a);
}

// 0x06 - LD B, (d8)
pub fn ld_b_d8(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.b = cpu.fetch8_timed(bus);
}

// 0x08 - LD (a16), SP
pub fn ld_a16_sp(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.bus_write_timed(bus, addr, (cpu.sp & 0xFF) as u8);
    cpu.bus_write_timed(bus, addr.wrapping_add(1), (cpu.sp >> 8) as u8);
}

// 0x0A - LD A, (BC)
pub fn ld_a_bc(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.a = cpu.bus_read_timed(bus, cpu.get_bc());
}

// 0x0E - LD C, d8
pub fn ld_c_d8(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.c = cpu.fetch8_timed(bus);
}

// 0x11 - LD DE, (d16)
pub fn ld_de_d16(cpu: &mut Cpu, bus: &mut Bus) {
    let value = cpu.fetch16_timed(bus);
    cpu.set_de(value);
}

// 0x12 - LD (DE), A
pub fn ld_de_a(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_de(), cpu.regs.a);
}

// 0x16 - LD D, d8
pub fn ld_d_d8(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.d = cpu.fetch8_timed(bus);
}

// 0x1A - LD A, (DE)
pub fn ld_a_de(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_de();
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}

// 0x1E - LD E, d8
pub fn ld_e_d8(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.e = cpu.fetch8_timed(bus);
}

// 0x21 - LD HL, (d16)
pub fn ld_hl_d16(cpu: &mut Cpu, bus: &mut Bus) {
    let value = cpu.fetch16_timed(bus);
    cpu.set_hl(value);
}

// 0x22 - LD (HL)+, A
pub fn ld_hl_plus_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
    cpu.set_hl(addr.wrapping_add(1));
}

// 0x26 - LD H, d8
pub fn ld_h_d8(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.h = cpu.fetch8_timed(bus);
}

// 0x2A - LD A, (HL+)
pub fn ld_a_hl_plus(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
    bus.maybe_trigger_oam_corruption(addr, OamCorruptionType::Read);
    cpu.set_hl(addr.wrapping_add(1));
}

// 0x2E - LD L, d8
pub fn ld_l_d8(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.l = cpu.fetch8_timed(bus);
}

// 0x31 - LD SP, (d16)
pub fn ld_sp_d16(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.sp = cpu.fetch16_timed(bus);
}

// 0x32 - LD (HL-), A
pub fn ld_hl_minus_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
    cpu.set_hl(addr.wrapping_sub(1));
}

// 0x36 - LD (HL), d8
pub fn ld_hl_d8(cpu: &mut Cpu, bus: &mut Bus) {
    let val = cpu.fetch8_timed(bus);
    cpu.bus_write_timed(bus, cpu.get_hl(), val);
}

// 0x3A - LD A, (HL-)
pub fn ld_a_hl_minus(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
    bus.maybe_trigger_oam_corruption(addr, OamCorruptionType::Read);
    cpu.set_hl(addr.wrapping_sub(1));
}

// 0x3E - LD A, (d8)
pub fn ld_a_d8(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.a = cpu.fetch8_timed(bus);
}

// 0x40 - LD B, B
pub fn ld_b_b(_bus: &mut Bus) {
    // No operation needed
}

// 0x41 - LD B, C
pub fn ld_b_c(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.b = cpu.regs.c;
}

// 0x42 - LD B, D
pub fn ld_b_d(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.b = cpu.regs.d;
}

// 0x43 - LD B, E
pub fn ld_b_e(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.b = cpu.regs.e;
}

// 0x44 - LD B, H
pub fn ld_b_h(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.b = cpu.regs.h;
}

// 0x45 - LD B, L
pub fn ld_b_l(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.b = cpu.regs.l;
}

// 0x46 - LD B, (HL)
pub fn ld_b_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.b = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x47 - LD B, A
pub fn ld_b_a(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.b = cpu.regs.a;
}

// 0x48 - LD C, B
pub fn ld_c_b(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.c = cpu.regs.b;
}

// 0x49 - LD C, C
pub fn ld_c_c(_bus: &mut Bus) {
    // No operation needed
}

// 0x4A - LD C, D
pub fn ld_c_d(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.c = cpu.regs.d;
}

// 0x4B - LD C, E
pub fn ld_c_e(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.c = cpu.regs.e;
}

// 0x4C - LD C, H
pub fn ld_c_h(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.c = cpu.regs.h;
}

// 0x4D - LD C, L
pub fn ld_c_l(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.c = cpu.regs.l;
}

// 0x4E - LD C, (HL)
pub fn ld_c_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.c = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x4F - LD C, A
pub fn ld_c_a(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.c = cpu.regs.a;
}

// 0x50 - LD D, B
pub fn ld_d_b(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.d = cpu.regs.b;
}

// 0x51 - LD D, C
pub fn ld_d_c(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.d = cpu.regs.c;
}

// 0x52 - LD D, D
pub fn ld_d_d(_cpu: &mut Cpu, _bus: &mut Bus) {
    // No operation needed
}

// 0x53 - LD D, E
pub fn ld_d_e(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.d = cpu.regs.e;
}

// 0x54 - LD D, H
pub fn ld_d_h(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.d = cpu.regs.h;
}

// 0x55 - LD D, L
pub fn ld_d_l(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.d = cpu.regs.l;
}

// 0x56 - LD D, (HL)
pub fn ld_d_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.d = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x57 - LD D, A
pub fn ld_d_a(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.d = cpu.regs.a;
}

// 0x58 - LD E, B
pub fn ld_e_b(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.e = cpu.regs.b;
}

// 0x59 - LD E, C
pub fn ld_e_c(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.e = cpu.regs.c;
}

// 0x5A - LD E, D
pub fn ld_e_d(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.e = cpu.regs.d;
}

// 0x5B - LD E, E
pub fn ld_e_e(_bus: &mut Bus) {
    // No operation needed
}

// 0x5C - LD E, H
pub fn ld_e_h(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.e = cpu.regs.h;
}

// 0x5D - LD E, L
pub fn ld_e_l(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.e = cpu.regs.l;
}

// 0x5E - LD E, (HL)
pub fn ld_e_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.e = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x5F - LD E, A
pub fn ld_e_a(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.e = cpu.regs.a;
}

// 0x60 - LD H, B
pub fn ld_h_b(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.h = cpu.regs.b;
}

// 0x61 - LD H, C
pub fn ld_h_c(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.h = cpu.regs.c;
}

// 0x62 - LD H, D
pub fn ld_h_d(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.h = cpu.regs.d;
}

// 0x63 - LD H, E
pub fn ld_h_e(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.h = cpu.regs.e;
}

// 0x64 - LD H, H
pub fn ld_h_h(_bus: &mut Bus) {
    // No operation needed
}

// 0x65 - LD H, L
pub fn ld_h_l(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.h = cpu.regs.l;
}

// 0x66 - LD H, (HL)
pub fn ld_h_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.h = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x67 - LD H, A
pub fn ld_h_a(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.h = cpu.regs.a;
}

// 0x68 - LD L, B
pub fn ld_l_b(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.l = cpu.regs.b;
}

// 0x69 - LD L, C
pub fn ld_l_c(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.l = cpu.regs.c;
}

// 0x6A - LD L, D
pub fn ld_l_d(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.l = cpu.regs.d;
}

// 0x6B - LD L, E
pub fn ld_l_e(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.l = cpu.regs.e;
}

// 0x6C - LD L, H
pub fn ld_l_h(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.l = cpu.regs.h;
}

// 0x6D - LD L, L
pub fn ld_l_l(_cpu: &mut Cpu, _bus: &mut Bus) {
    // No operation needed
}

// 0x6E - LD L, (HL)
pub fn ld_l_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.l = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x6F - LD L, A
pub fn ld_l_a(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.l = cpu.regs.a;
}

// 0x70 - LD (HL), B
pub fn ld_hl_b(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.b);
}

// 0x71 - LD (HL), C
pub fn ld_hl_c(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.c);
}

// 0x72 - LD (HL), D
pub fn ld_hl_d(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.d);
}

// 0x73 - LD (HL), E
pub fn ld_hl_e(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.e);
}

// 0x74 - LD (HL), H
pub fn ld_hl_h(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.h);
}

// 0x75 - LD (HL), L
pub fn ld_hl_l(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.l);
}

// 0x76 is HALT (Control Instruction) - Intentionally Omitted Here

// 0x77 - LD (HL), A
pub fn ld_hl_a(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.a);
}

// 0x78 - LD A, B
pub fn ld_a_b(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.b;
}

// 0x79 - LD A, C
pub fn ld_a_c(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.c;
}

// 0x7A - LD A, D
pub fn ld_a_d(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.d;
}

// 0x7B - LD A, E
pub fn ld_a_e(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.e;
}

// 0x7C - LD A, H
pub fn ld_a_h(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.h;
}

// 0x7D - LD A, L
pub fn ld_a_l(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.l;
}

// 0x7E - LD A, (HL)
pub fn ld_a_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.a = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x7F - LD A, A
pub fn ld_a_a(_cpu: &mut Cpu, _: &mut Bus) {
    // No operation needed
}

// 0xC1 - POP BC
pub fn pop_bc(cpu: &mut Cpu, bus: &mut Bus) {
    let value = cpu.pop16_timed_oam(bus);
    cpu.set_bc(value);
}

// 0xC5 - PUSH BC
pub fn push_bc(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed_oam(bus, cpu.get_bc());
}

// 0xD1 - POP DE
pub fn pop_de(cpu: &mut Cpu, bus: &mut Bus) {
    let value = cpu.pop16_timed_oam(bus);
    cpu.set_de(value);
}

// 0xD5 - PUSH DE
pub fn push_de(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed_oam(bus, cpu.get_de());
}

// 0xE0 - LDH (a8), A
pub fn ldh_a8_a(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus);
    let addr = memory_constants::IO_START | (offset as u16);
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
}

// 0xE1 - POP HL
pub fn pop_hl(cpu: &mut Cpu, bus: &mut Bus) {
    let value = cpu.pop16_timed_oam(bus);
    cpu.set_hl(value);
}

// 0xE2 - LD (0xFF00+C), A
pub fn ld_c_addr_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = 0xFF00 | (cpu.regs.c as u16);
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
}

// 0xE5 - PUSH HL
pub fn push_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed_oam(bus, cpu.get_hl());
}

// 0xEA - LD (a16), A
pub fn ld_a16_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
}

// 0xF0 - LDH A, (a8)
pub fn ldh_a_a8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus);
    let addr = memory_constants::IO_START | (offset as u16);
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}

// 0xF1 - POP AF
pub fn pop_af(cpu: &mut Cpu, bus: &mut Bus) {
    let value = cpu.pop16_timed_oam(bus);
    cpu.set_af(value);
}

// 0xF2 - LD A, (0xFF00+C)
pub fn ld_a_c_addr(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = 0xFF00 | (cpu.regs.c as u16);
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}

// 0xF5 - PUSH AF
pub fn push_af(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed_oam(bus, cpu.get_af());
}

// 0xF8 - LD HL, SP+r8
pub fn ld_hl_sp_r8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus) as i8 as i16 as u16;
    cpu.tick_internal_timed(bus, 4);
    let sp = cpu.sp;
    let result = sp.wrapping_add(offset);
    cpu.set_flags(
        false,
        false,
        (sp ^ offset ^ result) & 0x10 != 0,
        (sp ^ offset ^ result) & 0x100 != 0,
    );
    cpu.set_hl(result);
}

// 0xF9 - LD SP, HL
pub fn ld_sp_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.sp = cpu.get_hl();
}

// 0xFA - LD A, (a16)
pub fn ld_a_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}
