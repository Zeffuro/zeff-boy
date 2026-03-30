use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

// --- Non-CB-prefix rotate ops (0x07, 0x0F, 0x17, 0x1F) ---
// These always clear the Zero flag, unlike their CB-prefix counterparts.

// 0x07: RLCA
pub fn rlca(cpu: &mut Cpu, _bus: &mut Bus) {
    let carry = (cpu.regs.a & 0x80) != 0;
    cpu.regs.a = cpu.regs.a.rotate_left(1);
    cpu.set_flags(false, false, false, carry);
}

// 0x0F: RRCA
pub fn rrca(cpu: &mut Cpu, _bus: &mut Bus) {
    let carry = (cpu.regs.a & 0x01) != 0;
    cpu.regs.a = cpu.regs.a.rotate_right(1);
    cpu.set_flags(false, false, false, carry);
}

// 0x17: RLA
pub fn rla(cpu: &mut Cpu, _bus: &mut Bus) {
    let old_carry = cpu.get_c() as u8;
    let new_carry = (cpu.regs.a & 0x80) != 0;
    cpu.regs.a = (cpu.regs.a << 1) | old_carry;
    cpu.set_flags(false, false, false, new_carry);
}

// 0x1F: RRA
pub fn rra(cpu: &mut Cpu, _bus: &mut Bus) {
    let old_carry = if cpu.get_c() { 0x80 } else { 0 };
    let new_carry = (cpu.regs.a & 0x01) != 0;
    cpu.regs.a = (cpu.regs.a >> 1) | old_carry;
    cpu.set_flags(false, false, false, new_carry);
}

// ==========================================================================
// CB-prefix opcode execution (0xCB 0x00 - 0xCB 0xFF)
//
// Opcode byte layout:
//   Bits 7-6: group  (0=rotate/shift/swap, 1=BIT, 2=RES, 3=SET)
//   Bits 5-3: sub-op (shift type for group 0, or bit index for groups 1-3)
//   Bits 2-0: register index (0=B, 1=C, 2=D, 3=E, 4=H, 5=L, 6=(HL), 7=A)
//
// This replaces ~1,100 lines of individual functions with ~60 lines of
// decoded dispatch, while producing identical behavior and timing.
// ==========================================================================

/// Read a register value by CB-prefix register index.
/// Index 6 = (HL) read with bus timing.
#[inline(always)]
fn read_reg(cpu: &mut Cpu, bus: &mut Bus, idx: u8) -> u8 {
    match idx {
        0 => cpu.regs.b,
        1 => cpu.regs.c,
        2 => cpu.regs.d,
        3 => cpu.regs.e,
        4 => cpu.regs.h,
        5 => cpu.regs.l,
        6 => cpu.bus_read_timed(bus, cpu.get_hl()),
        7 => cpu.regs.a,
        _ => unreachable!(),
    }
}

/// Write a value to a register by CB-prefix register index.
/// Index 6 = (HL) write with bus timing.
#[inline(always)]
fn write_reg(cpu: &mut Cpu, bus: &mut Bus, idx: u8, val: u8) {
    match idx {
        0 => cpu.regs.b = val,
        1 => cpu.regs.c = val,
        2 => cpu.regs.d = val,
        3 => cpu.regs.e = val,
        4 => cpu.regs.h = val,
        5 => cpu.regs.l = val,
        6 => {
            let addr = cpu.get_hl();
            cpu.bus_write_timed(bus, addr, val);
        }
        7 => cpu.regs.a = val,
        _ => unreachable!(),
    }
}

/// Apply a rotate/shift/swap operation by sub-op index (0-7).
#[inline(always)]
fn apply_shift_op(cpu: &mut Cpu, sub_op: u8, val: u8) -> u8 {
    match sub_op {
        0 => cpu.rlc(val),
        1 => cpu.rrc(val),
        2 => cpu.rl(val),
        3 => cpu.rr(val),
        4 => cpu.sla(val),
        5 => cpu.sra(val),
        6 => cpu.swap(val),
        7 => cpu.srl(val),
        _ => unreachable!(),
    }
}

/// Execute a CB-prefixed opcode (called after the CB prefix byte is consumed).
pub fn execute_cb_op(cpu: &mut Cpu, bus: &mut Bus, opcode: u8) {
    let reg_idx = opcode & 0x07;
    let sub_op = (opcode >> 3) & 0x07;

    match opcode >> 6 {
        0 => {
            // 0x00-0x3F: Rotate/shift/swap:read, modify, write
            if reg_idx == 6 {
                let addr = cpu.get_hl();
                let val = cpu.bus_read_timed(bus, addr);
                let result = apply_shift_op(cpu, sub_op, val);
                cpu.bus_write_timed(bus, addr, result);
            } else {
                let val = read_reg(cpu, bus, reg_idx);
                let result = apply_shift_op(cpu, sub_op, val);
                write_reg(cpu, bus, reg_idx, result);
            }
        }
        1 => {
            // 0x40-0x7F: BIT:read-only test, no write-back
            let val = read_reg(cpu, bus, reg_idx);
            cpu.bit(sub_op, val);
        }
        2 => {
            // 0x80-0xBF: RES:read, clear bit, write
            if reg_idx == 6 {
                let addr = cpu.get_hl();
                let val = cpu.bus_read_timed(bus, addr);
                let result = cpu.res(sub_op, val);
                cpu.bus_write_timed(bus, addr, result);
            } else {
                let val = read_reg(cpu, bus, reg_idx);
                let result = cpu.res(sub_op, val);
                write_reg(cpu, bus, reg_idx, result);
            }
        }
        3 => {
            // 0xC0-0xFF: SET:read, set bit, write
            if reg_idx == 6 {
                let addr = cpu.get_hl();
                let val = cpu.bus_read_timed(bus, addr);
                let result = cpu.set(sub_op, val);
                cpu.bus_write_timed(bus, addr, result);
            } else {
                let val = read_reg(cpu, bus, reg_idx);
                let result = cpu.set(sub_op, val);
                write_reg(cpu, bus, reg_idx, result);
            }
        }
        _ => unreachable!(),
    }
}
