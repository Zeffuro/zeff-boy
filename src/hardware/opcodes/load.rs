use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;
use crate::hardware::types::constants as memory_constants;

// 0x01 - LD BC, (d16)
pub(crate) fn ld_bc_d16(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch16_timed(bus);
    cpu.set_bc(value);
}

// 0x02 - LD (BC), A
pub(crate) fn ld_bc_a(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_bc(), cpu.a);
}

// 0x06 - LD B, (d8)
pub(crate) fn ld_b_d8(cpu: &mut CPU, bus: &mut Bus) {
    cpu.b = cpu.fetch8_timed(bus);
}


// 0x08 - LD (a16), SP
pub(crate) fn ld_a16_sp(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.bus_write_timed(bus, addr, (cpu.sp & 0xFF) as u8);
    cpu.bus_write_timed(bus, addr.wrapping_add(1), (cpu.sp >> 8) as u8);
}

// 0x0A - LD A, (BC)
pub(crate) fn ld_a_bc(cpu: &mut CPU, bus: &mut Bus) {
    cpu.a = cpu.bus_read_timed(bus, cpu.get_bc());
}

// 0x0E - LD C, d8
pub(crate) fn ld_c_d8(cpu: &mut CPU, bus: &mut Bus) {
    cpu.c = cpu.fetch8_timed(bus);
}

// 0x11 - LD DE, (d16)
pub(crate) fn ld_de_d16(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch16_timed(bus);
    cpu.set_de(value);
}

// 0x12 - LD (DE), A
pub(crate) fn ld_de_a(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_de(), cpu.a);
}

// 0x16 - LD D, d8
pub(crate) fn ld_d_d8(cpu: &mut CPU, bus: &mut Bus) {
    cpu.d = cpu.fetch8_timed(bus);
}

// 0x1A - LD A, (DE)
pub(crate) fn ld_a_de(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_de();
    cpu.a = cpu.bus_read_timed(bus, addr);
}

// 0x1E - LD E, d8
pub(crate) fn ld_e_d8(cpu: &mut CPU, bus: &mut Bus) {
    cpu.e = cpu.fetch8_timed(bus);
}

// 0x21 - LD HL, (d16)
pub(crate) fn ld_hl_d16(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch16_timed(bus);
    cpu.set_hl(value);
}

// 0x22 - LD (HL)+, A
pub(crate) fn ld_hl_plus_a(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.bus_write_timed(bus, addr, cpu.a);
    cpu.set_hl(addr.wrapping_add(1));
}

// 0x26 - LD H, d8
pub(crate) fn ld_h_d8(cpu: &mut CPU, bus: &mut Bus) {
    cpu.h = cpu.fetch8_timed(bus);
}

// 0x2A - LD A, (HL+)
pub(crate) fn ld_a_hl_plus(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.a = cpu.bus_read_timed(bus, addr);
    cpu.set_hl(addr.wrapping_add(1));
}

// 0x2E - LD L, d8
pub(crate) fn ld_l_d8(cpu: &mut CPU, bus: &mut Bus) {
    cpu.l = cpu.fetch8_timed(bus);
}

// 0x31 - LD SP, (d16)
pub(crate) fn ld_sp_d16(cpu: &mut CPU, bus: &mut Bus) {
    cpu.sp = cpu.fetch16_timed(bus);
}

// 0x32 - LD (HL-), A
pub(crate) fn ld_hl_minus_a(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.bus_write_timed(bus, addr, cpu.a);
    cpu.set_hl(addr.wrapping_sub(1));
}

// 0x36 - LD (HL), d8
pub(crate) fn ld_hl_d8(cpu: &mut CPU, bus: &mut Bus) {
    let val = cpu.fetch8_timed(bus);
    cpu.bus_write_timed(bus, cpu.get_hl(), val);
}

// 0x3A - LD A, (HL-)
pub(crate) fn ld_a_hl_minus(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.a = cpu.bus_read_timed(bus, addr);
    cpu.set_hl(addr.wrapping_sub(1));
}

// 0x3E - LD A, (d8)
pub(crate) fn ld_a_d8(cpu: &mut CPU, bus: &mut Bus) {
    cpu.a = cpu.fetch8_timed(bus);
}

// 0x40 - LD B, B
pub(crate) fn ld_b_b(_bus: &mut Bus) {
    // No operation needed
}

// 0x41 - LD B, C
pub(crate) fn ld_b_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.c;
}

// 0x42 - LD B, D
pub(crate) fn ld_b_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.d;
}

// 0x43 - LD B, E
pub(crate) fn ld_b_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.e;
}

// 0x44 - LD B, H
pub(crate) fn ld_b_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.h;
}

// 0x45 - LD B, L
pub(crate) fn ld_b_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.l;
}

// 0x46 - LD B, (HL)
pub(crate) fn ld_b_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.b = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x47 - LD B, A
pub(crate) fn ld_b_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.a;
}

// 0x48 - LD C, B
pub(crate) fn ld_c_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.b;
}

// 0x49 - LD C, C
pub(crate) fn ld_c_c(_bus: &mut Bus) {
    // No operation needed
}

// 0x4A - LD C, D
pub(crate) fn ld_c_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.d;
}

// 0x4B - LD C, E
pub(crate) fn ld_c_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.e;
}

// 0x4C - LD C, H
pub(crate) fn ld_c_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.h;
}

// 0x4D - LD C, L
pub(crate) fn ld_c_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.l;
}

// 0x4E - LD C, (HL)
pub(crate) fn ld_c_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.c = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x4F - LD C, A
pub(crate) fn ld_c_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.a;
}

// 0x50 - LD D, B
pub(crate) fn ld_d_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.b;
}

// 0x51 - LD D, C
pub(crate) fn ld_d_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.c;
}

// 0x52 - LD D, D
pub(crate) fn ld_d_d(_cpu: &mut CPU, _bus: &mut Bus) {
    // No operation needed
}

// 0x53 - LD D, E
pub(crate) fn ld_d_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.e;
}

// 0x54 - LD D, H
pub(crate) fn ld_d_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.h;
}

// 0x55 - LD D, L
pub(crate) fn ld_d_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.l;
}

// 0x56 - LD D, (HL)
pub(crate) fn ld_d_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.d = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x57 - LD D, A
pub(crate) fn ld_d_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.a;
}

// 0x58 - LD E, B
pub(crate) fn ld_e_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.b;
}

// 0x59 - LD E, C
pub(crate) fn ld_e_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.c;
}

// 0x5A - LD E, D
pub(crate) fn ld_e_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.d;
}

// 0x5B - LD E, E
pub(crate) fn ld_e_e(_bus: &mut Bus) {
    // No operation needed
}

// 0x5C - LD E, H
pub(crate) fn ld_e_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.h;
}

// 0x5D - LD E, L
pub(crate) fn ld_e_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.l;
}

// 0x5E - LD E, (HL)
pub(crate) fn ld_e_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.e = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x5F - LD E, A
pub(crate) fn ld_e_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.a;
}

// 0x60 - LD H, B
pub(crate) fn ld_h_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.b;
}

// 0x61 - LD H, C
pub(crate) fn ld_h_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.c;
}

// 0x62 - LD H, D
pub(crate) fn ld_h_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.d;
}

// 0x63 - LD H, E
pub(crate) fn ld_h_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.e;
}

// 0x64 - LD H, H
pub(crate) fn ld_h_h(_bus: &mut Bus) {
    // No operation needed
}

// 0x65 - LD H, L
pub(crate) fn ld_h_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.l;
}

// 0x66 - LD H, (HL)
pub(crate) fn ld_h_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.h = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x67 - LD H, A
pub(crate) fn ld_h_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.a;
}

// 0x68 - LD L, B
pub(crate) fn ld_l_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.b;
}

// 0x69 - LD L, C
pub(crate) fn ld_l_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.c;
}

// 0x6A - LD L, D
pub(crate) fn ld_l_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.d;
}

// 0x6B - LD L, E
pub(crate) fn ld_l_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.e;
}

// 0x6C - LD L, H
pub(crate) fn ld_l_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.h;
}

// 0x6D - LD L, L
pub(crate) fn ld_l_l(_cpu: &mut CPU, _bus: &mut Bus) {
    // No operation needed
}

// 0x6E - LD L, (HL)
pub(crate) fn ld_l_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.l = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x6F - LD L, A
pub(crate) fn ld_l_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.a;
}

// 0x70 - LD (HL), B
pub(crate) fn ld_hl_b(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.b);
}

// 0x71 - LD (HL), C
pub(crate) fn ld_hl_c(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.c);
}

// 0x72 - LD (HL), D
pub(crate) fn ld_hl_d(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.d);
}

// 0x73 - LD (HL), E
pub(crate) fn ld_hl_e(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.e);
}

// 0x74 - LD (HL), H
pub(crate) fn ld_hl_h(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.h);
}

// 0x75 - LD (HL), L
pub(crate) fn ld_hl_l(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.l);
}

// 0x76 is HALT (Control Instruction) - Intentionally Omitted Here

// 0x77 - LD (HL), A
pub(crate) fn ld_hl_a(cpu: &mut CPU, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_hl(), cpu.a);
}

// 0x78 - LD A, B
pub(crate) fn ld_a_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.b;
}

// 0x79 - LD A, C
pub(crate) fn ld_a_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.c;
}

// 0x7A - LD A, D
pub(crate) fn ld_a_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.d;
}

// 0x7B - LD A, E
pub(crate) fn ld_a_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.e;
}

// 0x7C - LD A, H
pub(crate) fn ld_a_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.h;
}

// 0x7D - LD A, L
pub(crate) fn ld_a_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.l;
}

// 0x7E - LD A, (HL)
pub(crate) fn ld_a_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.a = cpu.bus_read_timed(bus, cpu.get_hl());
}

// 0x7F - LD A, A
pub(crate) fn ld_a_a(_cpu: &mut CPU, _: &mut Bus) {
    // No operation needed
}

// 0xC1 - POP BC
pub(crate) fn pop_bc(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.pop16_timed(bus);
    cpu.set_bc(value);
}

// 0xC5 - PUSH BC
pub(crate) fn push_bc(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.get_bc());
}

// 0xD1 - POP DE
pub(crate) fn pop_de(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.pop16_timed(bus);
    cpu.set_de(value);
}

// 0xD5 - PUSH DE
pub(crate) fn push_de(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.get_de());
}

// 0xE0 - LDH (a8), A
pub(crate) fn ldh_a8_a(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus);
    let addr = memory_constants::IO_START | (offset as u16);
    cpu.bus_write_timed(bus, addr, cpu.a);
}

// 0xE1 - POP HL
pub(crate) fn pop_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.pop16_timed(bus);
    cpu.set_hl(value);
}

// 0xE2 - LD (0xFF00+C), A
pub(crate) fn ld_c_addr_a(cpu: &mut CPU, bus: &mut Bus) {
    let addr = 0xFF00 | (cpu.c as u16);
    cpu.bus_write_timed(bus, addr, cpu.a);
}

// 0xE5 - PUSH HL
pub(crate) fn push_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.get_hl());
}

// 0xEA - LD (a16), A
pub(crate) fn ld_a16_a(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.bus_write_timed(bus, addr, cpu.a);
}

// 0xF0 - LDH A, (a8)
pub(crate) fn ldh_a_a8(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus);
    let addr = memory_constants::IO_START | (offset as u16);
    cpu.a = cpu.bus_read_timed(bus, addr);
}

// 0xF1 - POP AF
pub(crate) fn pop_af(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.pop16_timed(bus);
    cpu.set_af(value);
}

// 0xF2 - LD A, (0xFF00+C)
pub(crate) fn ld_a_c_addr(cpu: &mut CPU, bus: &mut Bus) {
    let addr = 0xFF00 | (cpu.c as u16);
    cpu.a = cpu.bus_read_timed(bus, addr);
}

// 0xF5 - PUSH AF
pub(crate) fn push_af(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.push16_timed(bus, cpu.get_af());
}

// 0xF8 - LD HL, SP+r8
pub(crate) fn ld_hl_sp_r8(cpu: &mut CPU, bus: &mut Bus) {
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
pub(crate) fn ld_sp_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.sp = cpu.get_hl();
}

// 0xFA - LD A, (a16)
pub(crate) fn ld_a_a16(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.a = cpu.bus_read_timed(bus, addr);
}