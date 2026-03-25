use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;

fn read_hl_timed(cpu: &mut CPU, bus: &mut Bus) -> u8 {
    cpu.bus_read_timed(bus, cpu.get_hl())
}

fn modify_hl_timed<F>(cpu: &mut CPU, bus: &mut Bus, f: F)
where
    F: FnOnce(&mut CPU, u8) -> u8,
{
    let addr = cpu.get_hl();
    let val = cpu.bus_read_timed(bus, addr);
    let result = f(cpu, val);
    cpu.bus_write_timed(bus, addr, result);
}

// 0x07: RLCA
pub fn rlca(cpu: &mut CPU, _bus: &mut Bus) {
    let carry = (cpu.regs.a & 0x80) != 0;
    cpu.regs.a = cpu.regs.a.rotate_left(1);
    cpu.set_flags(false, false, false, carry);
}

// 0x0F: RRCA
pub fn rrca(cpu: &mut CPU, _bus: &mut Bus) {
    let carry = (cpu.regs.a & 0x01) != 0;
    cpu.regs.a = cpu.regs.a.rotate_right(1);
    cpu.set_flags(false, false, false, carry);
}

// 0x17: RLA
pub fn rla(cpu: &mut CPU, _bus: &mut Bus) {
    let old_carry = cpu.get_c() as u8;
    let new_carry = (cpu.regs.a & 0x80) != 0;
    cpu.regs.a = (cpu.regs.a << 1) | old_carry;
    cpu.set_flags(false, false, false, new_carry);
}

// 0x1F: RRA
pub fn rra(cpu: &mut CPU, _bus: &mut Bus) {
    let old_carry = if cpu.get_c() { 0x80 } else { 0 };
    let new_carry = (cpu.regs.a & 0x01) != 0;
    cpu.regs.a = (cpu.regs.a >> 1) | old_carry;
    cpu.set_flags(false, false, false, new_carry);
}

// --- CB prefix: RLC ---

// 0xCB 00: RLC B
pub fn rlc_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.rlc(cpu.regs.b);
}

// 0xCB 01: RLC C
pub fn rlc_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.rlc(cpu.regs.c);
}

// 0xCB 02: RLC D
pub fn rlc_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.rlc(cpu.regs.d);
}

// 0xCB 03: RLC E
pub fn rlc_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.rlc(cpu.regs.e);
}

// 0xCB 04: RLC H
pub fn rlc_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.rlc(cpu.regs.h);
}

// 0xCB 05: RLC L
pub fn rlc_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.rlc(cpu.regs.l);
}

// 0xCB 06: RLC (HL)
pub fn rlc_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.rlc(val));
}

// 0xCB 07: RLC A
pub fn rlc_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.rlc(cpu.regs.a);
}

// --- CB prefix: RRC ---

// 0xCB 08: RRC B
pub fn rrc_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.rrc(cpu.regs.b);
}

// 0xCB 09: RRC C
pub fn rrc_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.rrc(cpu.regs.c);
}

// 0xCB 0A: RRC D
pub fn rrc_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.rrc(cpu.regs.d);
}

// 0xCB 0B: RRC E
pub fn rrc_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.rrc(cpu.regs.e);
}

// 0xCB 0C: RRC H
pub fn rrc_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.rrc(cpu.regs.h);
}

// 0xCB 0D: RRC L
pub fn rrc_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.rrc(cpu.regs.l);
}

// 0xCB 0E: RRC (HL)
pub fn rrc_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.rrc(val));
}

// 0xCB 0F: RRC A
pub fn rrc_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.rrc(cpu.regs.a);
}

// --- CB prefix: RL ---

// 0xCB 10: RL B
pub fn rl_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.rl(cpu.regs.b);
}

// 0xCB 11: RL C
pub fn rl_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.rl(cpu.regs.c);
}

// 0xCB 12: RL D
pub fn rl_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.rl(cpu.regs.d);
}

// 0xCB 13: RL E
pub fn rl_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.rl(cpu.regs.e);
}

// 0xCB 14: RL H
pub fn rl_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.rl(cpu.regs.h);
}

// 0xCB 15: RL L
pub fn rl_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.rl(cpu.regs.l);
}

// 0xCB 16: RL (HL)
pub fn rl_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.rl(val));
}

// 0xCB 17: RL A
pub fn rl_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.rl(cpu.regs.a);
}

// --- CB prefix: RR ---

// 0xCB 18: RR B
pub fn rr_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.rr(cpu.regs.b);
}

// 0xCB 19: RR C
pub fn rr_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.rr(cpu.regs.c);
}

// 0xCB 1A: RR D
pub fn rr_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.rr(cpu.regs.d);
}

// 0xCB 1B: RR E
pub fn rr_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.rr(cpu.regs.e);
}

// 0xCB 1C: RR H
pub fn rr_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.rr(cpu.regs.h);
}

// 0xCB 1D: RR L
pub fn rr_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.rr(cpu.regs.l);
}

// 0xCB 1E: RR (HL)
pub fn rr_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.rr(val));
}

// 0xCB 1F: RR A
pub fn rr_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.rr(cpu.regs.a);
}

// --- CB prefix: SLA ---

// 0xCB 20: SLA B
pub fn sla_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.sla(cpu.regs.b);
}

// 0xCB 21: SLA C
pub fn sla_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.sla(cpu.regs.c);
}

// 0xCB 22: SLA D
pub fn sla_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.sla(cpu.regs.d);
}

// 0xCB 23: SLA E
pub fn sla_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.sla(cpu.regs.e);
}

// 0xCB 24: SLA H
pub fn sla_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.sla(cpu.regs.h);
}

// 0xCB 25: SLA L
pub fn sla_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.sla(cpu.regs.l);
}

// 0xCB 26: SLA (HL)
pub fn sla_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.sla(val));
}

// 0xCB 27: SLA A
pub fn sla_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.sla(cpu.regs.a);
}

// --- CB prefix: SRA ---

// 0xCB 28: SRA B
pub fn sra_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.sra(cpu.regs.b);
}

// 0xCB 29: SRA C
pub fn sra_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.sra(cpu.regs.c);
}

// 0xCB 2A: SRA D
pub fn sra_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.sra(cpu.regs.d);
}

// 0xCB 2B: SRA E
pub fn sra_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.sra(cpu.regs.e);
}

// 0xCB 2C: SRA H
pub fn sra_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.sra(cpu.regs.h);
}

// 0xCB 2D: SRA L
pub fn sra_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.sra(cpu.regs.l);
}

// 0xCB 2E: SRA (HL)
pub fn sra_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.sra(val));
}

// 0xCB 2F: SRA A
pub fn sra_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.sra(cpu.regs.a);
}

// --- CB prefix: SWAP ---

// 0xCB 30: SWAP B
pub fn swap_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.swap(cpu.regs.b);
}

// 0xCB 31: SWAP C
pub fn swap_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.swap(cpu.regs.c);
}

// 0xCB 32: SWAP D
pub fn swap_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.swap(cpu.regs.d);
}

// 0xCB 33: SWAP E
pub fn swap_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.swap(cpu.regs.e);
}

// 0xCB 34: SWAP H
pub fn swap_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.swap(cpu.regs.h);
}

// 0xCB 35: SWAP L
pub fn swap_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.swap(cpu.regs.l);
}

// 0xCB 36: SWAP (HL)
pub fn swap_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.swap(val));
}

// 0xCB 37: SWAP A
pub fn swap_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.swap(cpu.regs.a);
}

// --- CB prefix: SRL ---

// 0xCB 38: SRL B
pub fn srl_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.srl(cpu.regs.b);
}

// 0xCB 39: SRL C
pub fn srl_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.srl(cpu.regs.c);
}

// 0xCB 3A: SRL D
pub fn srl_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.srl(cpu.regs.d);
}

// 0xCB 3B: SRL E
pub fn srl_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.srl(cpu.regs.e);
}

// 0xCB 3C: SRL H
pub fn srl_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.srl(cpu.regs.h);
}

// 0xCB 3D: SRL L
pub fn srl_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.srl(cpu.regs.l);
}

// 0xCB 3E: SRL (HL)
pub fn srl_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, val| cpu.srl(val));
}

// 0xCB 3F: SRL A
pub fn srl_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.srl(cpu.regs.a);
}

// --- CB prefix: BIT ---

// 0xCB 40: BIT 0, B
pub fn bit_0_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(0, cpu.regs.b);
}
// 0xCB 41: BIT 0, C
pub fn bit_0_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(0, cpu.regs.c);
}
// 0xCB 42: BIT 0, D
pub fn bit_0_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(0, cpu.regs.d);
}
// 0xCB 43: BIT 0, E
pub fn bit_0_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(0, cpu.regs.e);
}
// 0xCB 44: BIT 0, H
pub fn bit_0_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(0, cpu.regs.h);
}
// 0xCB 45: BIT 0, L
pub fn bit_0_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(0, cpu.regs.l);
}
// 0xCB 46: BIT 0, (HL)
pub fn bit_0_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(0, val);
}
// 0xCB 47: BIT 0, A
pub fn bit_0_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(0, cpu.regs.a);
}

// 0xCB 48: BIT 1, B
pub fn bit_1_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(1, cpu.regs.b);
}
// 0xCB 49: BIT 1, C
pub fn bit_1_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(1, cpu.regs.c);
}
// 0xCB 4A: BIT 1, D
pub fn bit_1_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(1, cpu.regs.d);
}
// 0xCB 4B: BIT 1, E
pub fn bit_1_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(1, cpu.regs.e);
}
// 0xCB 4C: BIT 1, H
pub fn bit_1_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(1, cpu.regs.h);
}
// 0xCB 4D: BIT 1, L
pub fn bit_1_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(1, cpu.regs.l);
}
// 0xCB 4E: BIT 1, (HL)
pub fn bit_1_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(1, val);
}
// 0xCB 4F: BIT 1, A
pub fn bit_1_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(1, cpu.regs.a);
}

// 0xCB 50: BIT 2, B
pub fn bit_2_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(2, cpu.regs.b);
}
// 0xCB 51: BIT 2, C
pub fn bit_2_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(2, cpu.regs.c);
}
// 0xCB 52: BIT 2, D
pub fn bit_2_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(2, cpu.regs.d);
}
// 0xCB 53: BIT 2, E
pub fn bit_2_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(2, cpu.regs.e);
}
// 0xCB 54: BIT 2, H
pub fn bit_2_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(2, cpu.regs.h);
}
// 0xCB 55: BIT 2, L
pub fn bit_2_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(2, cpu.regs.l);
}
// 0xCB 56: BIT 2, (HL)
pub fn bit_2_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(2, val);
}
// 0xCB 57: BIT 2, A
pub fn bit_2_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(2, cpu.regs.a);
}

// 0xCB 58: BIT 3, B
pub fn bit_3_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(3, cpu.regs.b);
}
// 0xCB 59: BIT 3, C
pub fn bit_3_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(3, cpu.regs.c);
}
// 0xCB 5A: BIT 3, D
pub fn bit_3_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(3, cpu.regs.d);
}
// 0xCB 5B: BIT 3, E
pub fn bit_3_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(3, cpu.regs.e);
}
// 0xCB 5C: BIT 3, H
pub fn bit_3_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(3, cpu.regs.h);
}
// 0xCB 5D: BIT 3, L
pub fn bit_3_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(3, cpu.regs.l);
}
// 0xCB 5E: BIT 3, (HL)
pub fn bit_3_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(3, val);
}
// 0xCB 5F: BIT 3, A
pub fn bit_3_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(3, cpu.regs.a);
}

// 0xCB 60: BIT 4, B
pub fn bit_4_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(4, cpu.regs.b);
}
// 0xCB 61: BIT 4, C
pub fn bit_4_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(4, cpu.regs.c);
}
// 0xCB 62: BIT 4, D
pub fn bit_4_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(4, cpu.regs.d);
}
// 0xCB 63: BIT 4, E
pub fn bit_4_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(4, cpu.regs.e);
}
// 0xCB 64: BIT 4, H
pub fn bit_4_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(4, cpu.regs.h);
}
// 0xCB 65: BIT 4, L
pub fn bit_4_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(4, cpu.regs.l);
}
// 0xCB 66: BIT 4, (HL)
pub fn bit_4_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(4, val);
}
// 0xCB 67: BIT 4, A
pub fn bit_4_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(4, cpu.regs.a);
}

// 0xCB 68: BIT 5, B
pub fn bit_5_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(5, cpu.regs.b);
}
// 0xCB 69: BIT 5, C
pub fn bit_5_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(5, cpu.regs.c);
}
// 0xCB 6A: BIT 5, D
pub fn bit_5_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(5, cpu.regs.d);
}
// 0xCB 6B: BIT 5, E
pub fn bit_5_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(5, cpu.regs.e);
}
// 0xCB 6C: BIT 5, H
pub fn bit_5_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(5, cpu.regs.h);
}
// 0xCB 6D: BIT 5, L
pub fn bit_5_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(5, cpu.regs.l);
}
// 0xCB 6E: BIT 5, (HL)
pub fn bit_5_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(5, val);
}
// 0xCB 6F: BIT 5, A
pub fn bit_5_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(5, cpu.regs.a);
}

// 0xCB 70: BIT 6, B
pub fn bit_6_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(6, cpu.regs.b);
}
// 0xCB 71: BIT 6, C
pub fn bit_6_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(6, cpu.regs.c);
}
// 0xCB 72: BIT 6, D
pub fn bit_6_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(6, cpu.regs.d);
}
// 0xCB 73: BIT 6, E
pub fn bit_6_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(6, cpu.regs.e);
}
// 0xCB 74: BIT 6, H
pub fn bit_6_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(6, cpu.regs.h);
}
// 0xCB 75: BIT 6, L
pub fn bit_6_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(6, cpu.regs.l);
}
// 0xCB 76: BIT 6, (HL)
pub fn bit_6_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(6, val);
}
// 0xCB 77: BIT 6, A
pub fn bit_6_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(6, cpu.regs.a);
}

// 0xCB 78: BIT 7, B
pub fn bit_7_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(7, cpu.regs.b);
}
// 0xCB 79: BIT 7, C
pub fn bit_7_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(7, cpu.regs.c);
}
// 0xCB 7A: BIT 7, D
pub fn bit_7_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(7, cpu.regs.d);
}
// 0xCB 7B: BIT 7, E
pub fn bit_7_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(7, cpu.regs.e);
}
// 0xCB 7C: BIT 7, H
pub fn bit_7_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(7, cpu.regs.h);
}
// 0xCB 7D: BIT 7, L
pub fn bit_7_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(7, cpu.regs.l);
}
// 0xCB 7E: BIT 7, (HL)
pub fn bit_7_hl(cpu: &mut CPU, bus: &mut Bus) {
    let val = read_hl_timed(cpu, bus);
    cpu.bit(7, val);
}
// 0xCB 7F: BIT 7, A
pub fn bit_7_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.bit(7, cpu.regs.a);
}

// --- CB prefix: RES ---

// 0xCB 80: RES 0, B
pub fn res_0_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(0, cpu.regs.b);
}
// 0xCB 81: RES 0, C
pub fn res_0_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(0, cpu.regs.c);
}
// 0xCB 82: RES 0, D
pub fn res_0_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(0, cpu.regs.d);
}
// 0xCB 83: RES 0, E
pub fn res_0_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(0, cpu.regs.e);
}
// 0xCB 84: RES 0, H
pub fn res_0_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(0, cpu.regs.h);
}
// 0xCB 85: RES 0, L
pub fn res_0_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(0, cpu.regs.l);
}
// 0xCB 86: RES 0, (HL)
pub fn res_0_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(0, v));
}
// 0xCB 87: RES 0, A
pub fn res_0_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(0, cpu.regs.a);
}

// 0xCB 88: RES 1, B
pub fn res_1_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(1, cpu.regs.b);
}
// 0xCB 89: RES 1, C
pub fn res_1_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(1, cpu.regs.c);
}
// 0xCB 8A: RES 1, D
pub fn res_1_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(1, cpu.regs.d);
}
// 0xCB 8B: RES 1, E
pub fn res_1_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(1, cpu.regs.e);
}
// 0xCB 8C: RES 1, H
pub fn res_1_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(1, cpu.regs.h);
}
// 0xCB 8D: RES 1, L
pub fn res_1_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(1, cpu.regs.l);
}
// 0xCB 8E: RES 1, (HL)
pub fn res_1_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(1, v));
}
// 0xCB 8F: RES 1, A
pub fn res_1_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(1, cpu.regs.a);
}

// 0xCB 90: RES 2, B
pub fn res_2_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(2, cpu.regs.b);
}
// 0xCB 91: RES 2, C
pub fn res_2_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(2, cpu.regs.c);
}
// 0xCB 92: RES 2, D
pub fn res_2_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(2, cpu.regs.d);
}
// 0xCB 93: RES 2, E
pub fn res_2_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(2, cpu.regs.e);
}
// 0xCB 94: RES 2, H
pub fn res_2_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(2, cpu.regs.h);
}
// 0xCB 95: RES 2, L
pub fn res_2_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(2, cpu.regs.l);
}
// 0xCB 96: RES 2, (HL)
pub fn res_2_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(2, v));
}
// 0xCB 97: RES 2, A
pub fn res_2_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(2, cpu.regs.a);
}

// 0xCB 98: RES 3, B
pub fn res_3_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(3, cpu.regs.b);
}
// 0xCB 99: RES 3, C
pub fn res_3_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(3, cpu.regs.c);
}
// 0xCB 9A: RES 3, D
pub fn res_3_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(3, cpu.regs.d);
}
// 0xCB 9B: RES 3, E
pub fn res_3_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(3, cpu.regs.e);
}
// 0xCB 9C: RES 3, H
pub fn res_3_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(3, cpu.regs.h);
}
// 0xCB 9D: RES 3, L
pub fn res_3_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(3, cpu.regs.l);
}
// 0xCB 9E: RES 3, (HL)
pub fn res_3_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(3, v));
}
// 0xCB 9F: RES 3, A
pub fn res_3_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(3, cpu.regs.a);
}

// 0xCB A0: RES 4, B
pub fn res_4_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(4, cpu.regs.b);
}
// 0xCB A1: RES 4, C
pub fn res_4_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(4, cpu.regs.c);
}
// 0xCB A2: RES 4, D
pub fn res_4_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(4, cpu.regs.d);
}
// 0xCB A3: RES 4, E
pub fn res_4_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(4, cpu.regs.e);
}
// 0xCB A4: RES 4, H
pub fn res_4_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(4, cpu.regs.h);
}
// 0xCB A5: RES 4, L
pub fn res_4_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(4, cpu.regs.l);
}
// 0xCB A6: RES 4, (HL)
pub fn res_4_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(4, v));
}
// 0xCB A7: RES 4, A
pub fn res_4_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(4, cpu.regs.a);
}

// 0xCB A8: RES 5, B
pub fn res_5_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(5, cpu.regs.b);
}
// 0xCB A9: RES 5, C
pub fn res_5_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(5, cpu.regs.c);
}
// 0xCB AA: RES 5, D
pub fn res_5_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(5, cpu.regs.d);
}
// 0xCB AB: RES 5, E
pub fn res_5_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(5, cpu.regs.e);
}
// 0xCB AC: RES 5, H
pub fn res_5_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(5, cpu.regs.h);
}
// 0xCB AD: RES 5, L
pub fn res_5_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(5, cpu.regs.l);
}
// 0xCB AE: RES 5, (HL)
pub fn res_5_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(5, v));
}
// 0xCB AF: RES 5, A
pub fn res_5_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(5, cpu.regs.a);
}

// 0xCB B0: RES 6, B
pub fn res_6_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(6, cpu.regs.b);
}
// 0xCB B1: RES 6, C
pub fn res_6_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(6, cpu.regs.c);
}
// 0xCB B2: RES 6, D
pub fn res_6_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(6, cpu.regs.d);
}
// 0xCB B3: RES 6, E
pub fn res_6_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(6, cpu.regs.e);
}
// 0xCB B4: RES 6, H
pub fn res_6_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(6, cpu.regs.h);
}
// 0xCB B5: RES 6, L
pub fn res_6_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(6, cpu.regs.l);
}
// 0xCB B6: RES 6, (HL)
pub fn res_6_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(6, v));
}
// 0xCB B7: RES 6, A
pub fn res_6_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(6, cpu.regs.a);
}

// 0xCB B8: RES 7, B
pub fn res_7_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.res(7, cpu.regs.b);
}
// 0xCB B9: RES 7, C
pub fn res_7_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.res(7, cpu.regs.c);
}
// 0xCB BA: RES 7, D
pub fn res_7_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.res(7, cpu.regs.d);
}
// 0xCB BB: RES 7, E
pub fn res_7_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.res(7, cpu.regs.e);
}
// 0xCB BC: RES 7, H
pub fn res_7_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.res(7, cpu.regs.h);
}
// 0xCB BD: RES 7, L
pub fn res_7_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.res(7, cpu.regs.l);
}
// 0xCB BE: RES 7, (HL)
pub fn res_7_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.res(7, v));
}
// 0xCB BF: RES 7, A
pub fn res_7_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.res(7, cpu.regs.a);
}

// --- CB prefix: SET ---

// 0xCB C0: SET 0, B
pub fn set_0_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(0, cpu.regs.b);
}
// 0xCB C1: SET 0, C
pub fn set_0_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(0, cpu.regs.c);
}
// 0xCB C2: SET 0, D
pub fn set_0_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(0, cpu.regs.d);
}
// 0xCB C3: SET 0, E
pub fn set_0_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(0, cpu.regs.e);
}
// 0xCB C4: SET 0, H
pub fn set_0_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(0, cpu.regs.h);
}
// 0xCB C5: SET 0, L
pub fn set_0_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(0, cpu.regs.l);
}
// 0xCB C6: SET 0, (HL)
pub fn set_0_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(0, v));
}
// 0xCB C7: SET 0, A
pub fn set_0_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(0, cpu.regs.a);
}

// 0xCB C8: SET 1, B
pub fn set_1_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(1, cpu.regs.b);
}
// 0xCB C9: SET 1, C
pub fn set_1_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(1, cpu.regs.c);
}
// 0xCB CA: SET 1, D
pub fn set_1_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(1, cpu.regs.d);
}
// 0xCB CB: SET 1, E
pub fn set_1_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(1, cpu.regs.e);
}
// 0xCB CC: SET 1, H
pub fn set_1_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(1, cpu.regs.h);
}
// 0xCB CD: SET 1, L
pub fn set_1_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(1, cpu.regs.l);
}
// 0xCB CE: SET 1, (HL)
pub fn set_1_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(1, v));
}
// 0xCB CF: SET 1, A
pub fn set_1_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(1, cpu.regs.a);
}

// 0xCB D0: SET 2, B
pub fn set_2_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(2, cpu.regs.b);
}
// 0xCB D1: SET 2, C
pub fn set_2_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(2, cpu.regs.c);
}
// 0xCB D2: SET 2, D
pub fn set_2_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(2, cpu.regs.d);
}
// 0xCB D3: SET 2, E
pub fn set_2_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(2, cpu.regs.e);
}
// 0xCB D4: SET 2, H
pub fn set_2_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(2, cpu.regs.h);
}
// 0xCB D5: SET 2, L
pub fn set_2_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(2, cpu.regs.l);
}
// 0xCB D6: SET 2, (HL)
pub fn set_2_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(2, v));
}
// 0xCB D7: SET 2, A
pub fn set_2_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(2, cpu.regs.a);
}

// 0xCB D8: SET 3, B
pub fn set_3_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(3, cpu.regs.b);
}
// 0xCB D9: SET 3, C
pub fn set_3_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(3, cpu.regs.c);
}
// 0xCB DA: SET 3, D
pub fn set_3_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(3, cpu.regs.d);
}
// 0xCB DB: SET 3, E
pub fn set_3_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(3, cpu.regs.e);
}
// 0xCB DC: SET 3, H
pub fn set_3_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(3, cpu.regs.h);
}
// 0xCB DD: SET 3, L
pub fn set_3_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(3, cpu.regs.l);
}
// 0xCB DE: SET 3, (HL)
pub fn set_3_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(3, v));
}
// 0xCB DF: SET 3, A
pub fn set_3_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(3, cpu.regs.a);
}

// 0xCB E0: SET 4, B
pub fn set_4_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(4, cpu.regs.b);
}
// 0xCB E1: SET 4, C
pub fn set_4_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(4, cpu.regs.c);
}
// 0xCB E2: SET 4, D
pub fn set_4_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(4, cpu.regs.d);
}
// 0xCB E3: SET 4, E
pub fn set_4_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(4, cpu.regs.e);
}
// 0xCB E4: SET 4, H
pub fn set_4_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(4, cpu.regs.h);
}
// 0xCB E5: SET 4, L
pub fn set_4_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(4, cpu.regs.l);
}
// 0xCB E6: SET 4, (HL)
pub fn set_4_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(4, v));
}
// 0xCB E7: SET 4, A
pub fn set_4_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(4, cpu.regs.a);
}

// 0xCB E8: SET 5, B
pub fn set_5_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(5, cpu.regs.b);
}
// 0xCB E9: SET 5, C
pub fn set_5_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(5, cpu.regs.c);
}
// 0xCB EA: SET 5, D
pub fn set_5_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(5, cpu.regs.d);
}
// 0xCB EB: SET 5, E
pub fn set_5_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(5, cpu.regs.e);
}
// 0xCB EC: SET 5, H
pub fn set_5_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(5, cpu.regs.h);
}
// 0xCB ED: SET 5, L
pub fn set_5_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(5, cpu.regs.l);
}
// 0xCB EE: SET 5, (HL)
pub fn set_5_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(5, v));
}
// 0xCB EF: SET 5, A
pub fn set_5_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(5, cpu.regs.a);
}

// 0xCB F0: SET 6, B
pub fn set_6_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(6, cpu.regs.b);
}
// 0xCB F1: SET 6, C
pub fn set_6_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(6, cpu.regs.c);
}
// 0xCB F2: SET 6, D
pub fn set_6_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(6, cpu.regs.d);
}
// 0xCB F3: SET 6, E
pub fn set_6_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(6, cpu.regs.e);
}
// 0xCB F4: SET 6, H
pub fn set_6_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(6, cpu.regs.h);
}
// 0xCB F5: SET 6, L
pub fn set_6_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(6, cpu.regs.l);
}
// 0xCB F6: SET 6, (HL)
pub fn set_6_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(6, v));
}
// 0xCB F7: SET 6, A
pub fn set_6_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(6, cpu.regs.a);
}

// 0xCB F8: SET 7, B
pub fn set_7_b(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.b = cpu.set(7, cpu.regs.b);
}
// 0xCB F9: SET 7, C
pub fn set_7_c(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.c = cpu.set(7, cpu.regs.c);
}
// 0xCB FA: SET 7, D
pub fn set_7_d(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.d = cpu.set(7, cpu.regs.d);
}
// 0xCB FB: SET 7, E
pub fn set_7_e(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.e = cpu.set(7, cpu.regs.e);
}
// 0xCB FC: SET 7, H
pub fn set_7_h(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.h = cpu.set(7, cpu.regs.h);
}
// 0xCB FD: SET 7, L
pub fn set_7_l(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.l = cpu.set(7, cpu.regs.l);
}
// 0xCB FE: SET 7, (HL)
pub fn set_7_hl(cpu: &mut CPU, bus: &mut Bus) {
    modify_hl_timed(cpu, bus, |cpu, v| cpu.set(7, v));
}
// 0xCB FF: SET 7, A
pub fn set_7_a(cpu: &mut CPU, _bus: &mut Bus) {
    cpu.regs.a = cpu.set(7, cpu.regs.a);
}
