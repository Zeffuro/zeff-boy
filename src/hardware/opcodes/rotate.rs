use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;

// 0x07: RLCA
pub(crate) fn rlca(cpu: &mut CPU, _bus: &mut Bus) {
    let carry = (cpu.a & 0x80) != 0;
    cpu.a = cpu.a.rotate_left(1);
    cpu.set_flags(false, false, false, carry);
}

// 0x0F: RRCA
pub(crate) fn rrca(cpu: &mut CPU, _bus: &mut Bus) {
    let carry = (cpu.a & 0x01) != 0;
    cpu.a = cpu.a.rotate_right(1);
    cpu.set_flags(false, false, false, carry);
}

// 0x17: RLA
pub(crate) fn rla(cpu: &mut CPU, _bus: &mut Bus) {
    let old_carry = cpu.get_c() as u8;
    let new_carry = (cpu.a & 0x80) != 0;
    cpu.a = (cpu.a << 1) | old_carry;
    cpu.set_flags(false, false, false, new_carry);
}

// 0x1F: RRA
pub(crate) fn rra(cpu: &mut CPU, _bus: &mut Bus) {
    let old_carry = if cpu.get_c() { 0x80 } else { 0 };
    let new_carry = (cpu.a & 0x01) != 0;
    cpu.a = (cpu.a >> 1) | old_carry;
    cpu.set_flags(false, false, false, new_carry);
}

// --- CB prefix: RLC ---

// 0xCB 00: RLC B
pub(crate) fn rlc_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.rlc(cpu.b);
}

// 0xCB 01: RLC C
pub(crate) fn rlc_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.rlc(cpu.c);
}

// 0xCB 02: RLC D
pub(crate) fn rlc_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.rlc(cpu.d);
}

// 0xCB 03: RLC E
pub(crate) fn rlc_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.rlc(cpu.e);
}

// 0xCB 04: RLC H
pub(crate) fn rlc_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.rlc(cpu.h);
}

// 0xCB 05: RLC L
pub(crate) fn rlc_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.rlc(cpu.l);
}

// 0xCB 06: RLC (HL)
pub(crate) fn rlc_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.rlc(val);
    bus.write_byte(addr, result);
}

// 0xCB 07: RLC A
pub(crate) fn rlc_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.rlc(cpu.a);
}

// --- CB prefix: RRC ---

// 0xCB 08: RRC B
pub(crate) fn rrc_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.rrc(cpu.b);
}

// 0xCB 09: RRC C
pub(crate) fn rrc_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.rrc(cpu.c);
}

// 0xCB 0A: RRC D
pub(crate) fn rrc_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.rrc(cpu.d);
}

// 0xCB 0B: RRC E
pub(crate) fn rrc_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.rrc(cpu.e);
}

// 0xCB 0C: RRC H
pub(crate) fn rrc_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.rrc(cpu.h);
}

// 0xCB 0D: RRC L
pub(crate) fn rrc_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.rrc(cpu.l);
}

// 0xCB 0E: RRC (HL)
pub(crate) fn rrc_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.rrc(val);
    bus.write_byte(addr, result);
}

// 0xCB 0F: RRC A
pub(crate) fn rrc_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.rrc(cpu.a);
}

// --- CB prefix: RL ---

// 0xCB 10: RL B
pub(crate) fn rl_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.rl(cpu.b);
}

// 0xCB 11: RL C
pub(crate) fn rl_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.rl(cpu.c);
}

// 0xCB 12: RL D
pub(crate) fn rl_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.rl(cpu.d);
}

// 0xCB 13: RL E
pub(crate) fn rl_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.rl(cpu.e);
}

// 0xCB 14: RL H
pub(crate) fn rl_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.rl(cpu.h);
}

// 0xCB 15: RL L
pub(crate) fn rl_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.rl(cpu.l);
}

// 0xCB 16: RL (HL)
pub(crate) fn rl_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.rl(val);
    bus.write_byte(addr, result);
}

// 0xCB 17: RL A
pub(crate) fn rl_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.rl(cpu.a);
}

// --- CB prefix: RR ---

// 0xCB 18: RR B
pub(crate) fn rr_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.rr(cpu.b);
}

// 0xCB 19: RR C
pub(crate) fn rr_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.rr(cpu.c);
}

// 0xCB 1A: RR D
pub(crate) fn rr_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.rr(cpu.d);
}

// 0xCB 1B: RR E
pub(crate) fn rr_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.rr(cpu.e);
}

// 0xCB 1C: RR H
pub(crate) fn rr_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.rr(cpu.h);
}

// 0xCB 1D: RR L
pub(crate) fn rr_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.rr(cpu.l);
}

// 0xCB 1E: RR (HL)
pub(crate) fn rr_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.rr(val);
    bus.write_byte(addr, result);
}

// 0xCB 1F: RR A
pub(crate) fn rr_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.rr(cpu.a);
}

// --- CB prefix: SLA ---

// 0xCB 20: SLA B
pub(crate) fn sla_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.sla(cpu.b);
}

// 0xCB 21: SLA C
pub(crate) fn sla_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.sla(cpu.c);
}

// 0xCB 22: SLA D
pub(crate) fn sla_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.sla(cpu.d);
}

// 0xCB 23: SLA E
pub(crate) fn sla_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.sla(cpu.e);
}

// 0xCB 24: SLA H
pub(crate) fn sla_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.sla(cpu.h);
}

// 0xCB 25: SLA L
pub(crate) fn sla_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.sla(cpu.l);
}

// 0xCB 26: SLA (HL)
pub(crate) fn sla_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.sla(val);
    bus.write_byte(addr, result);
}

// 0xCB 27: SLA A
pub(crate) fn sla_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.sla(cpu.a);
}

// --- CB prefix: SRA ---

// 0xCB 28: SRA B
pub(crate) fn sra_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.sra(cpu.b);
}

// 0xCB 29: SRA C
pub(crate) fn sra_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.sra(cpu.c);
}

// 0xCB 2A: SRA D
pub(crate) fn sra_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.sra(cpu.d);
}

// 0xCB 2B: SRA E
pub(crate) fn sra_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.sra(cpu.e);
}

// 0xCB 2C: SRA H
pub(crate) fn sra_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.sra(cpu.h);
}

// 0xCB 2D: SRA L
pub(crate) fn sra_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.sra(cpu.l);
}

// 0xCB 2E: SRA (HL)
pub(crate) fn sra_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.sra(val);
    bus.write_byte(addr, result);
}

// 0xCB 2F: SRA A
pub(crate) fn sra_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.sra(cpu.a);
}

// --- CB prefix: SWAP ---

// 0xCB 30: SWAP B
pub(crate) fn swap_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.swap(cpu.b);
}

// 0xCB 31: SWAP C
pub(crate) fn swap_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.swap(cpu.c);
}

// 0xCB 32: SWAP D
pub(crate) fn swap_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.swap(cpu.d);
}

// 0xCB 33: SWAP E
pub(crate) fn swap_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.swap(cpu.e);
}

// 0xCB 34: SWAP H
pub(crate) fn swap_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.swap(cpu.h);
}

// 0xCB 35: SWAP L
pub(crate) fn swap_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.swap(cpu.l);
}

// 0xCB 36: SWAP (HL)
pub(crate) fn swap_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.swap(val);
    bus.write_byte(addr, result);
}

// 0xCB 37: SWAP A
pub(crate) fn swap_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.swap(cpu.a);
}

// --- CB prefix: SRL ---

// 0xCB 38: SRL B
pub(crate) fn srl_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.b = cpu.srl(cpu.b);
}

// 0xCB 39: SRL C
pub(crate) fn srl_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.c = cpu.srl(cpu.c);
}

// 0xCB 3A: SRL D
pub(crate) fn srl_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.d = cpu.srl(cpu.d);
}

// 0xCB 3B: SRL E
pub(crate) fn srl_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.e = cpu.srl(cpu.e);
}

// 0xCB 3C: SRL H
pub(crate) fn srl_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.h = cpu.srl(cpu.h);
}

// 0xCB 3D: SRL L
pub(crate) fn srl_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.l = cpu.srl(cpu.l);
}

// 0xCB 3E: SRL (HL)
pub(crate) fn srl_hl(cpu: &mut CPU, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = bus.read_byte(addr);
    let result = cpu.srl(val);
    bus.write_byte(addr, result);
}

// 0xCB 3F: SRL A
pub(crate) fn srl_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.a = cpu.srl(cpu.a);
}

// --- CB prefix: BIT ---

// 0xCB 40: BIT 0, B
pub(crate) fn bit_0_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(0, cpu.b); }
// 0xCB 41: BIT 0, C
pub(crate) fn bit_0_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(0, cpu.c); }
// 0xCB 42: BIT 0, D
pub(crate) fn bit_0_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(0, cpu.d); }
// 0xCB 43: BIT 0, E
pub(crate) fn bit_0_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(0, cpu.e); }
// 0xCB 44: BIT 0, H
pub(crate) fn bit_0_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(0, cpu.h); }
// 0xCB 45: BIT 0, L
pub(crate) fn bit_0_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(0, cpu.l); }
// 0xCB 46: BIT 0, (HL)
pub(crate) fn bit_0_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(0, bus.read_byte(cpu.get_hl())); }
// 0xCB 47: BIT 0, A
pub(crate) fn bit_0_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(0, cpu.a); }

// 0xCB 48: BIT 1, B
pub(crate) fn bit_1_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(1, cpu.b); }
// 0xCB 49: BIT 1, C
pub(crate) fn bit_1_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(1, cpu.c); }
// 0xCB 4A: BIT 1, D
pub(crate) fn bit_1_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(1, cpu.d); }
// 0xCB 4B: BIT 1, E
pub(crate) fn bit_1_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(1, cpu.e); }
// 0xCB 4C: BIT 1, H
pub(crate) fn bit_1_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(1, cpu.h); }
// 0xCB 4D: BIT 1, L
pub(crate) fn bit_1_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(1, cpu.l); }
// 0xCB 4E: BIT 1, (HL)
pub(crate) fn bit_1_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(1, bus.read_byte(cpu.get_hl())); }
// 0xCB 4F: BIT 1, A
pub(crate) fn bit_1_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(1, cpu.a); }

// 0xCB 50: BIT 2, B
pub(crate) fn bit_2_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(2, cpu.b); }
// 0xCB 51: BIT 2, C
pub(crate) fn bit_2_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(2, cpu.c); }
// 0xCB 52: BIT 2, D
pub(crate) fn bit_2_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(2, cpu.d); }
// 0xCB 53: BIT 2, E
pub(crate) fn bit_2_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(2, cpu.e); }
// 0xCB 54: BIT 2, H
pub(crate) fn bit_2_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(2, cpu.h); }
// 0xCB 55: BIT 2, L
pub(crate) fn bit_2_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(2, cpu.l); }
// 0xCB 56: BIT 2, (HL)
pub(crate) fn bit_2_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(2, bus.read_byte(cpu.get_hl())); }
// 0xCB 57: BIT 2, A
pub(crate) fn bit_2_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(2, cpu.a); }

// 0xCB 58: BIT 3, B
pub(crate) fn bit_3_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(3, cpu.b); }
// 0xCB 59: BIT 3, C
pub(crate) fn bit_3_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(3, cpu.c); }
// 0xCB 5A: BIT 3, D
pub(crate) fn bit_3_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(3, cpu.d); }
// 0xCB 5B: BIT 3, E
pub(crate) fn bit_3_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(3, cpu.e); }
// 0xCB 5C: BIT 3, H
pub(crate) fn bit_3_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(3, cpu.h); }
// 0xCB 5D: BIT 3, L
pub(crate) fn bit_3_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(3, cpu.l); }
// 0xCB 5E: BIT 3, (HL)
pub(crate) fn bit_3_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(3, bus.read_byte(cpu.get_hl())); }
// 0xCB 5F: BIT 3, A
pub(crate) fn bit_3_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(3, cpu.a); }

// 0xCB 60: BIT 4, B
pub(crate) fn bit_4_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(4, cpu.b); }
// 0xCB 61: BIT 4, C
pub(crate) fn bit_4_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(4, cpu.c); }
// 0xCB 62: BIT 4, D
pub(crate) fn bit_4_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(4, cpu.d); }
// 0xCB 63: BIT 4, E
pub(crate) fn bit_4_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(4, cpu.e); }
// 0xCB 64: BIT 4, H
pub(crate) fn bit_4_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(4, cpu.h); }
// 0xCB 65: BIT 4, L
pub(crate) fn bit_4_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(4, cpu.l); }
// 0xCB 66: BIT 4, (HL)
pub(crate) fn bit_4_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(4, bus.read_byte(cpu.get_hl())); }
// 0xCB 67: BIT 4, A
pub(crate) fn bit_4_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(4, cpu.a); }

// 0xCB 68: BIT 5, B
pub(crate) fn bit_5_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(5, cpu.b); }
// 0xCB 69: BIT 5, C
pub(crate) fn bit_5_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(5, cpu.c); }
// 0xCB 6A: BIT 5, D
pub(crate) fn bit_5_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(5, cpu.d); }
// 0xCB 6B: BIT 5, E
pub(crate) fn bit_5_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(5, cpu.e); }
// 0xCB 6C: BIT 5, H
pub(crate) fn bit_5_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(5, cpu.h); }
// 0xCB 6D: BIT 5, L
pub(crate) fn bit_5_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(5, cpu.l); }
// 0xCB 6E: BIT 5, (HL)
pub(crate) fn bit_5_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(5, bus.read_byte(cpu.get_hl())); }
// 0xCB 6F: BIT 5, A
pub(crate) fn bit_5_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(5, cpu.a); }

// 0xCB 70: BIT 6, B
pub(crate) fn bit_6_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(6, cpu.b); }
// 0xCB 71: BIT 6, C
pub(crate) fn bit_6_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(6, cpu.c); }
// 0xCB 72: BIT 6, D
pub(crate) fn bit_6_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(6, cpu.d); }
// 0xCB 73: BIT 6, E
pub(crate) fn bit_6_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(6, cpu.e); }
// 0xCB 74: BIT 6, H
pub(crate) fn bit_6_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(6, cpu.h); }
// 0xCB 75: BIT 6, L
pub(crate) fn bit_6_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(6, cpu.l); }
// 0xCB 76: BIT 6, (HL)
pub(crate) fn bit_6_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(6, bus.read_byte(cpu.get_hl())); }
// 0xCB 77: BIT 6, A
pub(crate) fn bit_6_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(6, cpu.a); }

// 0xCB 78: BIT 7, B
pub(crate) fn bit_7_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(7, cpu.b); }
// 0xCB 79: BIT 7, C
pub(crate) fn bit_7_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(7, cpu.c); }
// 0xCB 7A: BIT 7, D
pub(crate) fn bit_7_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(7, cpu.d); }
// 0xCB 7B: BIT 7, E
pub(crate) fn bit_7_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(7, cpu.e); }
// 0xCB 7C: BIT 7, H
pub(crate) fn bit_7_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(7, cpu.h); }
// 0xCB 7D: BIT 7, L
pub(crate) fn bit_7_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(7, cpu.l); }
// 0xCB 7E: BIT 7, (HL)
pub(crate) fn bit_7_hl(cpu: &mut CPU, bus: &mut Bus) { cpu.bit(7, bus.read_byte(cpu.get_hl())); }
// 0xCB 7F: BIT 7, A
pub(crate) fn bit_7_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.bit(7, cpu.a); }

// --- CB prefix: RES ---

// 0xCB 80: RES 0, B
pub(crate) fn res_0_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(0, cpu.b); }
// 0xCB 81: RES 0, C
pub(crate) fn res_0_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(0, cpu.c); }
// 0xCB 82: RES 0, D
pub(crate) fn res_0_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(0, cpu.d); }
// 0xCB 83: RES 0, E
pub(crate) fn res_0_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(0, cpu.e); }
// 0xCB 84: RES 0, H
pub(crate) fn res_0_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(0, cpu.h); }
// 0xCB 85: RES 0, L
pub(crate) fn res_0_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(0, cpu.l); }
// 0xCB 86: RES 0, (HL)
pub(crate) fn res_0_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(0, v)); }
// 0xCB 87: RES 0, A
pub(crate) fn res_0_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(0, cpu.a); }

// 0xCB 88: RES 1, B
pub(crate) fn res_1_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(1, cpu.b); }
// 0xCB 89: RES 1, C
pub(crate) fn res_1_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(1, cpu.c); }
// 0xCB 8A: RES 1, D
pub(crate) fn res_1_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(1, cpu.d); }
// 0xCB 8B: RES 1, E
pub(crate) fn res_1_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(1, cpu.e); }
// 0xCB 8C: RES 1, H
pub(crate) fn res_1_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(1, cpu.h); }
// 0xCB 8D: RES 1, L
pub(crate) fn res_1_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(1, cpu.l); }
// 0xCB 8E: RES 1, (HL)
pub(crate) fn res_1_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(1, v)); }
// 0xCB 8F: RES 1, A
pub(crate) fn res_1_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(1, cpu.a); }

// 0xCB 90: RES 2, B
pub(crate) fn res_2_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(2, cpu.b); }
// 0xCB 91: RES 2, C
pub(crate) fn res_2_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(2, cpu.c); }
// 0xCB 92: RES 2, D
pub(crate) fn res_2_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(2, cpu.d); }
// 0xCB 93: RES 2, E
pub(crate) fn res_2_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(2, cpu.e); }
// 0xCB 94: RES 2, H
pub(crate) fn res_2_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(2, cpu.h); }
// 0xCB 95: RES 2, L
pub(crate) fn res_2_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(2, cpu.l); }
// 0xCB 96: RES 2, (HL)
pub(crate) fn res_2_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(2, v)); }
// 0xCB 97: RES 2, A
pub(crate) fn res_2_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(2, cpu.a); }

// 0xCB 98: RES 3, B
pub(crate) fn res_3_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(3, cpu.b); }
// 0xCB 99: RES 3, C
pub(crate) fn res_3_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(3, cpu.c); }
// 0xCB 9A: RES 3, D
pub(crate) fn res_3_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(3, cpu.d); }
// 0xCB 9B: RES 3, E
pub(crate) fn res_3_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(3, cpu.e); }
// 0xCB 9C: RES 3, H
pub(crate) fn res_3_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(3, cpu.h); }
// 0xCB 9D: RES 3, L
pub(crate) fn res_3_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(3, cpu.l); }
// 0xCB 9E: RES 3, (HL)
pub(crate) fn res_3_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(3, v)); }
// 0xCB 9F: RES 3, A
pub(crate) fn res_3_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(3, cpu.a); }

// 0xCB A0: RES 4, B
pub(crate) fn res_4_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(4, cpu.b); }
// 0xCB A1: RES 4, C
pub(crate) fn res_4_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(4, cpu.c); }
// 0xCB A2: RES 4, D
pub(crate) fn res_4_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(4, cpu.d); }
// 0xCB A3: RES 4, E
pub(crate) fn res_4_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(4, cpu.e); }
// 0xCB A4: RES 4, H
pub(crate) fn res_4_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(4, cpu.h); }
// 0xCB A5: RES 4, L
pub(crate) fn res_4_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(4, cpu.l); }
// 0xCB A6: RES 4, (HL)
pub(crate) fn res_4_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(4, v)); }
// 0xCB A7: RES 4, A
pub(crate) fn res_4_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(4, cpu.a); }

// 0xCB A8: RES 5, B
pub(crate) fn res_5_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(5, cpu.b); }
// 0xCB A9: RES 5, C
pub(crate) fn res_5_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(5, cpu.c); }
// 0xCB AA: RES 5, D
pub(crate) fn res_5_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(5, cpu.d); }
// 0xCB AB: RES 5, E
pub(crate) fn res_5_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(5, cpu.e); }
// 0xCB AC: RES 5, H
pub(crate) fn res_5_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(5, cpu.h); }
// 0xCB AD: RES 5, L
pub(crate) fn res_5_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(5, cpu.l); }
// 0xCB AE: RES 5, (HL)
pub(crate) fn res_5_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(5, v)); }
// 0xCB AF: RES 5, A
pub(crate) fn res_5_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(5, cpu.a); }

// 0xCB B0: RES 6, B
pub(crate) fn res_6_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(6, cpu.b); }
// 0xCB B1: RES 6, C
pub(crate) fn res_6_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(6, cpu.c); }
// 0xCB B2: RES 6, D
pub(crate) fn res_6_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(6, cpu.d); }
// 0xCB B3: RES 6, E
pub(crate) fn res_6_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(6, cpu.e); }
// 0xCB B4: RES 6, H
pub(crate) fn res_6_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(6, cpu.h); }
// 0xCB B5: RES 6, L
pub(crate) fn res_6_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(6, cpu.l); }
// 0xCB B6: RES 6, (HL)
pub(crate) fn res_6_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(6, v)); }
// 0xCB B7: RES 6, A
pub(crate) fn res_6_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(6, cpu.a); }

// 0xCB B8: RES 7, B
pub(crate) fn res_7_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.res(7, cpu.b); }
// 0xCB B9: RES 7, C
pub(crate) fn res_7_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.res(7, cpu.c); }
// 0xCB BA: RES 7, D
pub(crate) fn res_7_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.res(7, cpu.d); }
// 0xCB BB: RES 7, E
pub(crate) fn res_7_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.res(7, cpu.e); }
// 0xCB BC: RES 7, H
pub(crate) fn res_7_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.res(7, cpu.h); }
// 0xCB BD: RES 7, L
pub(crate) fn res_7_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.res(7, cpu.l); }
// 0xCB BE: RES 7, (HL)
pub(crate) fn res_7_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.res(7, v)); }
// 0xCB BF: RES 7, A
pub(crate) fn res_7_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.res(7, cpu.a); }

// --- CB prefix: SET ---

// 0xCB C0: SET 0, B
pub(crate) fn set_0_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(0, cpu.b); }
// 0xCB C1: SET 0, C
pub(crate) fn set_0_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(0, cpu.c); }
// 0xCB C2: SET 0, D
pub(crate) fn set_0_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(0, cpu.d); }
// 0xCB C3: SET 0, E
pub(crate) fn set_0_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(0, cpu.e); }
// 0xCB C4: SET 0, H
pub(crate) fn set_0_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(0, cpu.h); }
// 0xCB C5: SET 0, L
pub(crate) fn set_0_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(0, cpu.l); }
// 0xCB C6: SET 0, (HL)
pub(crate) fn set_0_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(0, v)); }
// 0xCB C7: SET 0, A
pub(crate) fn set_0_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(0, cpu.a); }

// 0xCB C8: SET 1, B
pub(crate) fn set_1_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(1, cpu.b); }
// 0xCB C9: SET 1, C
pub(crate) fn set_1_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(1, cpu.c); }
// 0xCB CA: SET 1, D
pub(crate) fn set_1_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(1, cpu.d); }
// 0xCB CB: SET 1, E
pub(crate) fn set_1_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(1, cpu.e); }
// 0xCB CC: SET 1, H
pub(crate) fn set_1_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(1, cpu.h); }
// 0xCB CD: SET 1, L
pub(crate) fn set_1_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(1, cpu.l); }
// 0xCB CE: SET 1, (HL)
pub(crate) fn set_1_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(1, v)); }
// 0xCB CF: SET 1, A
pub(crate) fn set_1_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(1, cpu.a); }

// 0xCB D0: SET 2, B
pub(crate) fn set_2_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(2, cpu.b); }
// 0xCB D1: SET 2, C
pub(crate) fn set_2_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(2, cpu.c); }
// 0xCB D2: SET 2, D
pub(crate) fn set_2_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(2, cpu.d); }
// 0xCB D3: SET 2, E
pub(crate) fn set_2_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(2, cpu.e); }
// 0xCB D4: SET 2, H
pub(crate) fn set_2_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(2, cpu.h); }
// 0xCB D5: SET 2, L
pub(crate) fn set_2_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(2, cpu.l); }
// 0xCB D6: SET 2, (HL)
pub(crate) fn set_2_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(2, v)); }
// 0xCB D7: SET 2, A
pub(crate) fn set_2_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(2, cpu.a); }

// 0xCB D8: SET 3, B
pub(crate) fn set_3_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(3, cpu.b); }
// 0xCB D9: SET 3, C
pub(crate) fn set_3_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(3, cpu.c); }
// 0xCB DA: SET 3, D
pub(crate) fn set_3_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(3, cpu.d); }
// 0xCB DB: SET 3, E
pub(crate) fn set_3_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(3, cpu.e); }
// 0xCB DC: SET 3, H
pub(crate) fn set_3_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(3, cpu.h); }
// 0xCB DD: SET 3, L
pub(crate) fn set_3_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(3, cpu.l); }
// 0xCB DE: SET 3, (HL)
pub(crate) fn set_3_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(3, v)); }
// 0xCB DF: SET 3, A
pub(crate) fn set_3_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(3, cpu.a); }

// 0xCB E0: SET 4, B
pub(crate) fn set_4_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(4, cpu.b); }
// 0xCB E1: SET 4, C
pub(crate) fn set_4_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(4, cpu.c); }
// 0xCB E2: SET 4, D
pub(crate) fn set_4_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(4, cpu.d); }
// 0xCB E3: SET 4, E
pub(crate) fn set_4_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(4, cpu.e); }
// 0xCB E4: SET 4, H
pub(crate) fn set_4_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(4, cpu.h); }
// 0xCB E5: SET 4, L
pub(crate) fn set_4_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(4, cpu.l); }
// 0xCB E6: SET 4, (HL)
pub(crate) fn set_4_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(4, v)); }
// 0xCB E7: SET 4, A
pub(crate) fn set_4_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(4, cpu.a); }

// 0xCB E8: SET 5, B
pub(crate) fn set_5_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(5, cpu.b); }
// 0xCB E9: SET 5, C
pub(crate) fn set_5_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(5, cpu.c); }
// 0xCB EA: SET 5, D
pub(crate) fn set_5_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(5, cpu.d); }
// 0xCB EB: SET 5, E
pub(crate) fn set_5_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(5, cpu.e); }
// 0xCB EC: SET 5, H
pub(crate) fn set_5_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(5, cpu.h); }
// 0xCB ED: SET 5, L
pub(crate) fn set_5_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(5, cpu.l); }
// 0xCB EE: SET 5, (HL)
pub(crate) fn set_5_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(5, v)); }
// 0xCB EF: SET 5, A
pub(crate) fn set_5_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(5, cpu.a); }

// 0xCB F0: SET 6, B
pub(crate) fn set_6_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(6, cpu.b); }
// 0xCB F1: SET 6, C
pub(crate) fn set_6_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(6, cpu.c); }
// 0xCB F2: SET 6, D
pub(crate) fn set_6_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(6, cpu.d); }
// 0xCB F3: SET 6, E
pub(crate) fn set_6_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(6, cpu.e); }
// 0xCB F4: SET 6, H
pub(crate) fn set_6_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(6, cpu.h); }
// 0xCB F5: SET 6, L
pub(crate) fn set_6_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(6, cpu.l); }
// 0xCB F6: SET 6, (HL)
pub(crate) fn set_6_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(6, v)); }
// 0xCB F7: SET 6, A
pub(crate) fn set_6_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(6, cpu.a); }

// 0xCB F8: SET 7, B
pub(crate) fn set_7_b(cpu: &mut CPU, _bus: &mut Bus) { cpu.b = cpu.set(7, cpu.b); }
// 0xCB F9: SET 7, C
pub(crate) fn set_7_c(cpu: &mut CPU, _bus: &mut Bus) { cpu.c = cpu.set(7, cpu.c); }
// 0xCB FA: SET 7, D
pub(crate) fn set_7_d(cpu: &mut CPU, _bus: &mut Bus) { cpu.d = cpu.set(7, cpu.d); }
// 0xCB FB: SET 7, E
pub(crate) fn set_7_e(cpu: &mut CPU, _bus: &mut Bus) { cpu.e = cpu.set(7, cpu.e); }
// 0xCB FC: SET 7, H
pub(crate) fn set_7_h(cpu: &mut CPU, _bus: &mut Bus) { cpu.h = cpu.set(7, cpu.h); }
// 0xCB FD: SET 7, L
pub(crate) fn set_7_l(cpu: &mut CPU, _bus: &mut Bus) { cpu.l = cpu.set(7, cpu.l); }
// 0xCB FE: SET 7, (HL)
pub(crate) fn set_7_hl(cpu: &mut CPU, bus: &mut Bus) { let a = cpu.get_hl(); let v = bus.read_byte(a); bus.write_byte(a, cpu.set(7, v)); }
// 0xCB FF: SET 7, A
pub(crate) fn set_7_a(cpu: &mut CPU, _bus: &mut Bus) { cpu.a = cpu.set(7, cpu.a); }
