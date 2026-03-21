use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;

// 0x03: INC BC
pub(crate) fn inc_bc(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.get_bc().wrapping_add(1);
    cpu.set_bc(value);
}

// 0x04: INC B
pub(crate) fn inc_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.inc(cpu.b);
}

// 0x05: DEC B
pub(crate) fn dec_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.dec(cpu.b);
}

// 0x09: ADD HL, BC
pub(crate) fn add_hl_bc(cpu: &mut CPU, bus: &mut Bus) {
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
pub(crate) fn dec_bc(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.get_bc().wrapping_sub(1);
    cpu.set_bc(value);
}

// 0x0C: INC C
pub(crate) fn inc_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.inc(cpu.c);
}

// 0x0D: DEC C
pub(crate) fn dec_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.dec(cpu.c);
}

// 0x13: INC DE
pub(crate) fn inc_de(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.get_de().wrapping_add(1);
    cpu.set_de(value);
}

// 0x14: INC D
pub(crate) fn inc_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.inc(cpu.d);
}

// 0x15: DEC D
pub(crate) fn dec_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.dec(cpu.d);
}

// 0x19: ADD HL, DE
pub(crate) fn add_hl_de(cpu: &mut CPU, bus: &mut Bus) {
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
pub(crate) fn dec_de(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.get_de().wrapping_sub(1);
    cpu.set_de(value);
}

// 0x1C: INC E
pub(crate) fn inc_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.inc(cpu.e);
}

// 0x1D: DEC E
pub(crate) fn dec_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.dec(cpu.e);
}

// 0x23: INC HL
pub(crate) fn inc_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.get_hl().wrapping_add(1);
    cpu.set_hl(value);
}

// 0x24: INC H
pub(crate) fn inc_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.inc(cpu.h);
}

// 0x25: DEC H
pub(crate) fn dec_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.dec(cpu.h);
}

// 0x27: DAA
pub(crate) fn daa(cpu: &mut CPU, _bus: &mut Bus) {
    let mut a = cpu.a;
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
    cpu.a = a;
}

// 0x29: ADD HL, HL
pub(crate) fn add_hl_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let hl = cpu.get_hl();
    let result = hl.wrapping_add(hl);
    cpu.set_n(false);
    cpu.set_h((hl & 0x0FFF) + (hl & 0x0FFF) > 0x0FFF);
    cpu.set_c((hl as u32) + (hl as u32) > 0xFFFF);
    cpu.set_hl(result);
}

// 0x2B: DEC HL
pub(crate) fn dec_hl(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.get_hl().wrapping_sub(1);
    cpu.set_hl(value);
}

// 0x2C: INC L
pub(crate) fn inc_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.inc(cpu.l);
}

// 0x2D: DEC L
pub(crate) fn dec_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.dec(cpu.l);
}

// 0x2F: CPL - Complement A
pub(crate) fn cpl(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = !cpu.a;
    cpu.set_n(true);
    cpu.set_h(true);
}

// 0x33: INC SP
pub(crate) fn inc_sp(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.sp.wrapping_add(1);
    cpu.sp = value;
}

// 0x34: INC (HL)
pub(crate) fn inc_hl_val(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = cpu.bus_read_timed(bus, addr);
    let new_val = cpu.inc(val);
    cpu.bus_write_timed(bus, addr, new_val);
}

// 0x35: DEC (HL)
pub(crate) fn dec_hl_val(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = cpu.bus_read_timed(bus, addr);
    let new_val = cpu.dec(val);
    cpu.bus_write_timed(bus, addr, new_val);
}

// 0x37: SCF - Set Carry Flag
pub(crate) fn scf(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.set_n(false);
    cpu.set_h(false);
    cpu.set_c(true);
}

// 0x39: ADD HL, SP
pub(crate) fn add_hl_sp(cpu: &mut CPU, bus: &mut Bus) {
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
pub(crate) fn dec_sp(cpu: &mut CPU, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let value = cpu.sp.wrapping_sub(1);
    cpu.sp = value;
}

// 0x3C: INC A
pub(crate) fn inc_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.inc(cpu.a);
}

// 0x3D: DEC A
pub(crate) fn dec_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.dec(cpu.a);
}

// 0x3F: CCF - Complement Carry Flag
pub(crate) fn ccf(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.set_n(false);
    cpu.set_h(false);
    cpu.set_c(!cpu.get_c());
}

// 0x80: ADD B
pub(crate) fn add_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.b);
}

// 0x81: ADD C
pub(crate) fn add_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.c);
}

// 0x82: ADD D
pub(crate) fn add_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.d);
}

// 0x83: ADD E
pub(crate) fn add_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.e);
}

// 0x84: ADD H
pub(crate) fn add_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.h);
}

// 0x85: ADD L
pub(crate) fn add_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.l);
}

// 0x86: ADD (HL)
pub(crate) fn add_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.add(value);
}

// 0x87: ADD A
pub(crate) fn add_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.add(cpu.a);
}

// 0x88: ADC A, B
pub(crate) fn adc_a_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.b);
}

// 0x89: ADC A, C
pub(crate) fn adc_a_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.c);
}

// 0x8A: ADC A, D
pub(crate) fn adc_a_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.d);
}

// 0x8B: ADC A, E
pub(crate) fn adc_a_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.e);
}

// 0x8C: ADC A, H
pub(crate) fn adc_a_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.h);
}

// 0x8D: ADC A, L
pub(crate) fn adc_a_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.adc(cpu.l);
}

// 0x8E: ADC A, (HL)
pub(crate) fn adc_a_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.adc(value);
}

// 0x8F: ADC A, A
pub(crate) fn adc_a_a(cpu: &mut CPU, _bus: &mut Bus) {
    let a = cpu.a;
    cpu.adc(a);
}

// 0x90: SUB B
pub(crate) fn sub_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.b);
}

// 0x91: SUB C
pub(crate) fn sub_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.c);
}

// 0x92: SUB D
pub(crate) fn sub_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.d);
}

// 0x93: SUB E
pub(crate) fn sub_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.e);
}

// 0x94: SUB H
pub(crate) fn sub_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.h);
}

// 0x95: SUB L
pub(crate) fn sub_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.l);
}

// 0x96: SUB (HL)
pub(crate) fn sub_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.sub(value);
}

// 0x97: SUB A
pub(crate) fn sub_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sub(cpu.a);
}

// 0x98: SBC A, B
pub(crate) fn sbc_a_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.b);
}

// 0x99: SBC A, C
pub(crate) fn sbc_a_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.c);
}

// 0x9A: SBC A, D
pub(crate) fn sbc_a_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.d);
}

// 0x9B: SBC A, E
pub(crate) fn sbc_a_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.e);
}

// 0x9C: SBC A, H
pub(crate) fn sbc_a_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.h);
}

// 0x9D: SBC A, L
pub(crate) fn sbc_a_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.l);
}

// 0x9E: SBC A, (HL)
pub(crate) fn sbc_a_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.sbc(value);
}

// 0x9F: SBC A, A
pub(crate) fn sbc_a_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.sbc(cpu.a);
}

// 0xA0: AND B
pub(crate) fn and_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.b);
}

// 0xA1: AND C
pub(crate) fn and_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.c);
}

// 0xA2: AND D
pub(crate) fn and_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.d);
}

// 0xA3: AND E
pub(crate) fn and_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.e);
}

// 0xA4: AND H
pub(crate) fn and_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.h);
}

// 0xA5: AND L
pub(crate) fn and_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.l);
}

// 0xA6: AND (HL)
pub(crate) fn and_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.logical_and(value);
}

// 0xA7: AND A
pub(crate) fn and_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_and(cpu.a);
}

// 0xA8: XOR B
pub(crate) fn xor_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.b);
}

// 0xA9: XOR C
pub(crate) fn xor_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.c);
}

// 0xAA: XOR D
pub(crate) fn xor_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.d);
}

// 0xAB: XOR E
pub(crate) fn xor_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.e);
}

// 0xAC: XOR H
pub(crate) fn xor_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.h);
}

// 0xAD: XOR L
pub(crate) fn xor_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.l);
}

// 0xAE: XOR (HL)
pub(crate) fn xor_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.logical_xor(value);
}

// 0xAF: XOR A
pub(crate) fn xor_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_xor(cpu.a);
}

// 0xB0: OR B
pub(crate) fn or_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.b);
}

// 0xB1: OR C
pub(crate) fn or_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.c);
}

// 0xB2: OR D
pub(crate) fn or_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.d);
}

// 0xB3: OR E
pub(crate) fn or_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.e);
}

// 0xB4: OR H
pub(crate) fn or_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.h);
}

// 0xB5: OR L
pub(crate) fn or_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.l);
}

// 0xB6: OR (HL)
pub(crate) fn or_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.logical_or(value);
}

// 0xB7: OR A
pub(crate) fn or_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.logical_or(cpu.a);
}

// 0xB8: CP B
pub(crate) fn cp_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.b);
}

// 0xB9: CP C
pub(crate) fn cp_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.c);
}

// 0xBA: CP D
pub(crate) fn cp_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.d);
}

// 0xBB: CP E
pub(crate) fn cp_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.e);
}

// 0xBC: CP H
pub(crate) fn cp_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.h);
}

// 0xBD: CP L
pub(crate) fn cp_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.l);
}

// 0xBE: CP (HL)
pub(crate) fn cp_hl(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.bus_read_timed(bus, cpu.get_hl());
    cpu.compare(value);
}

// 0xBF: CP A
pub(crate) fn cp_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.compare(cpu.a);
}

// 0xC6: ADC A, (d8)
pub(crate) fn add_a_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.add(value);
}

// 0xCE: ADC A, (d8)
pub(crate) fn adc_a_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.adc(value);
}

// 0xD6: SUB A, (d8)
pub(crate) fn sub_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.sub(value);
}

// 0xDE: SBC A, (d8)
pub(crate) fn sbc_a_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.sbc(value);
}

// 0xE6: AND (d8)
pub(crate) fn and_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.logical_and(value);
}

// 0xE8: ADD SP, r8
pub(crate) fn add_sp_r8(cpu: &mut CPU, bus: &mut Bus) {
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
pub(crate) fn xor_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.logical_xor(value);
}

// 0xF6: OR (d8)
pub(crate) fn or_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.logical_or(value);
}

// 0xFE: CP (d8)
pub(crate) fn cp_d8(cpu: &mut CPU, bus: &mut Bus) {
    let value = cpu.fetch8_timed(bus);
    cpu.compare(value);
}
