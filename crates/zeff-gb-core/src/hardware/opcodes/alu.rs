use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;

// 0x03: INC BC
pub fn inc_bc(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.inc_rp_timed(bus, cpu.get_bc());
    cpu.set_bc(value);
}

// 0x04: INC B
pub fn inc_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.inc(cpu.regs.b);
}

// 0x05: DEC B
pub fn dec_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.dec(cpu.regs.b);
}

// 0x09: ADD HL, BC
pub fn add_hl_bc(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let hl = cpu.get_hl();
    let bc = cpu.get_bc();
    let result = hl.wrapping_add(bc);
    cpu.set_n(false);
    cpu.set_h((hl & 0x0FFF) + (bc & 0x0FFF) > 0x0FFF);
    cpu.set_c((hl as u32) + (bc as u32) > 0xFFFF);
    cpu.set_hl(result);
}

// 0x0B: DEC BC
pub fn dec_bc(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.dec_rp_timed(bus, cpu.get_bc());
    cpu.set_bc(value);
}

// 0x0C: INC C
pub fn inc_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.inc(cpu.regs.c);
}

// 0x0D: DEC C
pub fn dec_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.dec(cpu.regs.c);
}

// 0x13: INC DE
pub fn inc_de(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.inc_rp_timed(bus, cpu.get_de());
    cpu.set_de(value);
}

// 0x14: INC D
pub fn inc_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.inc(cpu.regs.d);
}

// 0x15: DEC D
pub fn dec_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.dec(cpu.regs.d);
}

// 0x19: ADD HL, DE
pub fn add_hl_de(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let hl = cpu.get_hl();
    let de = cpu.get_de();
    let result = hl.wrapping_add(de);
    cpu.set_n(false);
    cpu.set_h((hl & 0x0FFF) + (de & 0x0FFF) > 0x0FFF);
    cpu.set_c((hl as u32) + (de as u32) > 0xFFFF);
    cpu.set_hl(result);
}

// 0x1B: DEC DE
pub fn dec_de(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.dec_rp_timed(bus, cpu.get_de());
    cpu.set_de(value);
}

// 0x1C: INC E
pub fn inc_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.inc(cpu.regs.e);
}

// 0x1D: DEC E
pub fn dec_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.dec(cpu.regs.e);
}

// 0x23: INC HL
pub fn inc_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.inc_rp_timed(bus, cpu.get_hl());
    cpu.set_hl(value);
}

// 0x24: INC H
pub fn inc_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.inc(cpu.regs.h);
}

// 0x25: DEC H
pub fn dec_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.dec(cpu.regs.h);
}

// 0x27: DAA
pub fn daa(cpu: &mut CPU, _bus: &mut Bus) {
    let mut a = cpu.regs.a;
    let mut adjust = if cpu.get_c() { 0x60 } else { 0x00 };

    if cpu.get_h() {
        adjust |= 0x06;
    }

    if !cpu.get_n() {
        if a & 0x0F > 0x09 {
            adjust |= 0x06;
        }
        if a > 0x99 {
            adjust |= 0x60;
        }
        a = a.wrapping_add(adjust);
    } else {
        a = a.wrapping_sub(adjust);
    }

    cpu.set_c(adjust >= 0x60);
    cpu.set_h(false);
    cpu.set_z(a == 0);
    cpu.regs.a = a;
}

// 0x29: ADD HL, HL
pub fn add_hl_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let hl = cpu.get_hl();
    let result = hl.wrapping_add(hl);
    cpu.set_n(false);
    cpu.set_h((hl & 0x0FFF) + (hl & 0x0FFF) > 0x0FFF);
    cpu.set_c((hl as u32) + (hl as u32) > 0xFFFF);
    cpu.set_hl(result);
}

// 0x2B: DEC HL
pub fn dec_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.dec_rp_timed(bus, cpu.get_hl());
    cpu.set_hl(value);
}

// 0x2C: INC L
pub fn inc_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.inc(cpu.regs.l);
}

// 0x2D: DEC L
pub fn dec_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.dec(cpu.regs.l);
}

// 0x2F: CPL - Complement A
pub fn cpl(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = !cpu.regs.a;
    cpu.set_n(true);
    cpu.set_h(true);
}

// 0x33: INC SP
pub fn inc_sp(cpu: &mut CPU, bus: &mut Bus) {
    cpu.sp = cpu.inc_rp_timed(bus, cpu.sp);
}

// 0x34: INC (HL)
pub fn inc_hl_val(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = cpu.bus_read_timed(bus, addr);
    let new_val = cpu.inc(val);
    cpu.bus_write_timed(bus, addr, new_val);
}

// 0x35: DEC (HL)
pub fn dec_hl_val(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = cpu.bus_read_timed(bus, addr);
    let new_val = cpu.dec(val);
    cpu.bus_write_timed(bus, addr, new_val);
}

// 0x37: SCF - Set Carry Flag
pub fn scf(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.set_n(false);
    cpu.set_h(false);
    cpu.set_c(true);
}

// 0x39: ADD HL, SP
pub fn add_hl_sp(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let hl = cpu.get_hl();
    let sp = cpu.sp;
    let result = hl.wrapping_add(sp);
    cpu.set_n(false);
    cpu.set_h((hl & 0x0FFF) + (sp & 0x0FFF) > 0x0FFF);
    cpu.set_c((hl as u32) + (sp as u32) > 0xFFFF);
    cpu.set_hl(result);
}

// 0x3B: DEC SP
pub fn dec_sp(cpu: &mut CPU, bus: &mut Bus) {
    cpu.sp = cpu.dec_rp_timed(bus, cpu.sp);
}

// 0x3C: INC A
pub fn inc_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.inc(cpu.regs.a);
}

// 0x3D: DEC A
pub fn dec_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.dec(cpu.regs.a);
}

// 0x3F: CCF - Complement Carry Flag
pub fn ccf(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.set_n(false);
    cpu.set_h(false);
    cpu.set_c(!cpu.get_c());
}

// 0x80: ADD B
pub fn add_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.regs.b);
}

// 0x81: ADD C
pub fn add_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.regs.c);
}

// 0x82: ADD D
pub fn add_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.regs.d);
}

// 0x83: ADD E
pub fn add_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.regs.e);
}

// 0x84: ADD H
pub fn add_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.regs.h);
}

// 0x85: ADD L
pub fn add_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.regs.l);
}

// 0x86: ADD (HL)
pub fn add_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.add(value);
}

// 0x87: ADD A
pub fn add_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.regs.a);
}

// 0x88: ADC A, B
pub fn adc_a_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.regs.b);
}

// 0x89: ADC A, C
pub fn adc_a_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.regs.c);
}

// 0x8A: ADC A, D
pub fn adc_a_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.regs.d);
}

// 0x8B: ADC A, E
pub fn adc_a_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.regs.e);
}

// 0x8C: ADC A, H
pub fn adc_a_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.regs.h);
}

// 0x8D: ADC A, L
pub fn adc_a_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.regs.l);
}

// 0x8E: ADC A, (HL)
pub fn adc_a_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.adc(value);
}

// 0x8F: ADC A, A
pub fn adc_a_a(cpu: &mut CPU, _bus: &mut Bus) {
    let a = cpu.regs.a;
    cpu.adc(a);
}

// 0x90: SUB B
pub fn sub_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.regs.b);
}

// 0x91: SUB C
pub fn sub_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.regs.c);
}

// 0x92: SUB D
pub fn sub_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.regs.d);
}

// 0x93: SUB E
pub fn sub_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.regs.e);
}

// 0x94: SUB H
pub fn sub_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.regs.h);
}

// 0x95: SUB L
pub fn sub_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.regs.l);
}

// 0x96: SUB (HL)
pub fn sub_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.sub(value);
}

// 0x97: SUB A
pub fn sub_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.regs.a);
}

// 0x98: SBC A, B
pub fn sbc_a_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.regs.b);
}

// 0x99: SBC A, C
pub fn sbc_a_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.regs.c);
}

// 0x9A: SBC A, D
pub fn sbc_a_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.regs.d);
}

// 0x9B: SBC A, E
pub fn sbc_a_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.regs.e);
}

// 0x9C: SBC A, H
pub fn sbc_a_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.regs.h);
}

// 0x9D: SBC A, L
pub fn sbc_a_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.regs.l);
}

// 0x9E: SBC A, (HL)
pub fn sbc_a_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.sbc(value);
}

// 0x9F: SBC A, A
pub fn sbc_a_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.regs.a);
}

// 0xA0: AND B
pub fn and_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.regs.b);
}

// 0xA1: AND C
pub fn and_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.regs.c);
}

// 0xA2: AND D
pub fn and_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.regs.d);
}

// 0xA3: AND E
pub fn and_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.regs.e);
}

// 0xA4: AND H
pub fn and_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.regs.h);
}

// 0xA5: AND L
pub fn and_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.regs.l);
}

// 0xA6: AND (HL)
pub fn and_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.logical_and(value);
}

// 0xA7: AND A
pub fn and_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.regs.a);
}

// 0xA8: XOR B
pub fn xor_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.regs.b);
}

// 0xA9: XOR C
pub fn xor_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.regs.c);
}

// 0xAA: XOR D
pub fn xor_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.regs.d);
}

// 0xAB: XOR E
pub fn xor_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.regs.e);
}

// 0xAC: XOR H
pub fn xor_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.regs.h);
}

// 0xAD: XOR L
pub fn xor_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.regs.l);
}

// 0xAE: XOR (HL)
pub fn xor_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.logical_xor(value);
}

// 0xAF: XOR A
pub fn xor_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.regs.a);
}

// 0xB0: OR B
pub fn or_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.regs.b);
}

// 0xB1: OR C
pub fn or_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.regs.c);
}

// 0xB2: OR D
pub fn or_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.regs.d);
}

// 0xB3: OR E
pub fn or_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.regs.e);
}

// 0xB4: OR H
pub fn or_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.regs.h);
}

// 0xB5: OR L
pub fn or_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.regs.l);
}

// 0xB6: OR (HL)
pub fn or_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.logical_or(value);
}

// 0xB7: OR A
pub fn or_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.regs.a);
}

// 0xB8: CP B
pub fn cp_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.regs.b);
}

// 0xB9: CP C
pub fn cp_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.regs.c);
}

// 0xBA: CP D
pub fn cp_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.regs.d);
}

// 0xBB: CP E
pub fn cp_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.regs.e);
}

// 0xBC: CP H
pub fn cp_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.regs.h);
}

// 0xBD: CP L
pub fn cp_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.regs.l);
}

// 0xBE: CP (HL)
pub fn cp_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.compare(value);
}

// 0xBF: CP A
pub fn cp_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.regs.a);
}

// 0xC6: ADC A, (d8)
pub fn add_a_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.add(value);
}

// 0xCE: ADC A, (d8)
pub fn adc_a_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.adc(value);
}

// 0xD6: SUB A, (d8)
pub fn sub_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.sub(value);
}

// 0xDE: SBC A, (d8)
pub fn sbc_a_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.sbc(value);
}

// 0xE6: AND (d8)
pub fn and_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.logical_and(value);
}

// 0xE8: ADD SP, r8
pub fn add_sp_r8(cpu: &mut CPU, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus) as i8 as i16 as u16;
    cpu.tick_internal_timed(bus, 8);
    let sp = cpu.sp;
    let result = sp.wrapping_add(offset);
    cpu.set_flags(
        false,
        false,
        (sp ^ offset ^ result) & 0x10 != 0,
        (sp ^ offset ^ result) & 0x100 != 0,
    );
    cpu.sp = result;
}

// 0xEE: XOR (d8)
pub fn xor_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.logical_xor(value);
}

// 0xF6: OR (d8)
pub fn or_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.logical_or(value);
}

// 0xFE: CP (d8)
pub fn cp_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.compare(value);
}
