use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::cpu::registers::StatusFlags;

#[inline(always)]
fn page_cross_penalty(crossed: bool) -> u8 {
    crossed as u8
}

// ── LAX: LDA + LDX ─────────────────────────────────────────────────

fn lax_set(cpu: &mut Cpu, val: u8) {
    cpu.regs.a = val;
    cpu.regs.x = val;
    cpu.regs.set_zn(val);
}

macro_rules! lax_modes {
    ($zp:ident, $zpy:ident, $abs:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_zero_page(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $zpy(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_zero_page_y(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_absolute(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $absy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (addr, crossed) = cpu.addr_absolute_y(bus);
            lax_set(cpu, bus.cpu_read(addr));
            page_cross_penalty(crossed)
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_indirect_x(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $indy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (addr, crossed) = cpu.addr_indirect_y(bus);
            lax_set(cpu, bus.cpu_read(addr));
            page_cross_penalty(crossed)
        }
    };
}

lax_modes!(lax_zp, lax_zp_y, lax_abs, lax_abs_y, lax_ind_x, lax_ind_y);

// ── SAX: store A & X ────────────────────────────────────────────────

macro_rules! sax_modes {
    ($zp:ident, $zpy:ident, $abs:ident, $indx:ident) => {
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_zero_page(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
        pub fn $zpy(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_zero_page_y(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_absolute(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_indirect_x(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
    };
}

sax_modes!(sax_zp, sax_zp_y, sax_abs, sax_ind_x);

// ── DCP: DEC + CMP ─────────────────────────────────────────────────

fn dcp_op(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let val = bus.cpu_read(addr).wrapping_sub(1);
    bus.cpu_write(addr, val);
    cpu.compare(cpu.regs.a, val);
}

// ── ISB (ISC): INC + SBC ───────────────────────────────────────────

fn isb_op(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let val = bus.cpu_read(addr).wrapping_add(1);
    bus.cpu_write(addr, val);
    cpu.sbc(val);
}

// ── SLO: ASL + ORA ─────────────────────────────────────────────────

fn slo_op(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let val = bus.cpu_read(addr);
    let shifted = cpu.asl_val(val);
    bus.cpu_write(addr, shifted);
    cpu.regs.a |= shifted;
    cpu.regs.set_zn(cpu.regs.a);
}

// ── RLA: ROL + AND ─────────────────────────────────────────────────

fn rla_op(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let val = bus.cpu_read(addr);
    let rotated = cpu.rol_val(val);
    bus.cpu_write(addr, rotated);
    cpu.regs.a &= rotated;
    cpu.regs.set_zn(cpu.regs.a);
}

// ── SRE: LSR + EOR ─────────────────────────────────────────────────

fn sre_op(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let val = bus.cpu_read(addr);
    let shifted = cpu.lsr_val(val);
    bus.cpu_write(addr, shifted);
    cpu.regs.a ^= shifted;
    cpu.regs.set_zn(cpu.regs.a);
}

// ── RRA: ROR + ADC ─────────────────────────────────────────────────

fn rra_op(cpu: &mut Cpu, bus: &mut Bus, addr: u16) {
    let val = bus.cpu_read(addr);
    let rotated = cpu.ror_val(val);
    bus.cpu_write(addr, rotated);
    cpu.adc(rotated);
}

macro_rules! rmw_unofficial_modes {
    ($op:ident, $zp:ident, $zpx:ident, $abs:ident,
     $absx:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_zero_page(bus);
            $op(cpu, bus, addr);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_zero_page_x(bus);
            $op(cpu, bus, addr);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_absolute(bus);
            $op(cpu, bus, addr);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) {
            let (addr, _) = cpu.addr_absolute_x(bus);
            $op(cpu, bus, addr);
        }
        pub fn $absy(cpu: &mut Cpu, bus: &mut Bus) {
            let (addr, _) = cpu.addr_absolute_y(bus);
            $op(cpu, bus, addr);
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let addr = cpu.addr_indirect_x(bus);
            $op(cpu, bus, addr);
        }
        pub fn $indy(cpu: &mut Cpu, bus: &mut Bus) {
            let (addr, _) = cpu.addr_indirect_y(bus);
            $op(cpu, bus, addr);
        }
    };
}

rmw_unofficial_modes!(
    dcp_op, dcp_zp, dcp_zp_x, dcp_abs, dcp_abs_x, dcp_abs_y, dcp_ind_x, dcp_ind_y
);

rmw_unofficial_modes!(
    isb_op, isb_zp, isb_zp_x, isb_abs, isb_abs_x, isb_abs_y, isb_ind_x, isb_ind_y
);

rmw_unofficial_modes!(
    slo_op, slo_zp, slo_zp_x, slo_abs, slo_abs_x, slo_abs_y, slo_ind_x, slo_ind_y
);

rmw_unofficial_modes!(
    rla_op, rla_zp, rla_zp_x, rla_abs, rla_abs_x, rla_abs_y, rla_ind_x, rla_ind_y
);

rmw_unofficial_modes!(
    sre_op, sre_zp, sre_zp_x, sre_abs, sre_abs_x, sre_abs_y, sre_ind_x, sre_ind_y
);

rmw_unofficial_modes!(
    rra_op, rra_zp, rra_zp_x, rra_abs, rra_abs_x, rra_abs_y, rra_ind_x, rra_ind_y
);

// ── Immediate-mode combined ops ─────────────────────────────────────

// ANC: AND #imm, then copy bit 7 of result to carry.
pub fn anc(cpu: &mut Cpu, bus: &Bus) {
    let addr = cpu.addr_immediate(bus);
    let val = bus.cpu_peek(addr);
    cpu.regs.a &= val;
    cpu.regs.set_zn(cpu.regs.a);
    cpu.regs
        .set_flag(StatusFlags::CARRY, cpu.regs.a & 0x80 != 0);
}

// ALR: AND #imm, then LSR A.
pub fn alr(cpu: &mut Cpu, bus: &Bus) {
    let addr = cpu.addr_immediate(bus);
    let val = bus.cpu_peek(addr);
    cpu.regs.a &= val;
    cpu.lsr_acc();
}

// ARR: AND #imm, then ROR A. Carry and overflow set specially.
pub fn arr(cpu: &mut Cpu, bus: &Bus) {
    let addr = cpu.addr_immediate(bus);
    let val = bus.cpu_peek(addr);
    cpu.regs.a &= val;
    let carry_in: u8 = if cpu.regs.get_flag(StatusFlags::CARRY) {
        0x80
    } else {
        0
    };
    cpu.regs.a = (cpu.regs.a >> 1) | carry_in;
    cpu.regs.set_zn(cpu.regs.a);
    let bit6 = (cpu.regs.a >> 6) & 1;
    let bit5 = (cpu.regs.a >> 5) & 1;
    cpu.regs.set_flag(StatusFlags::CARRY, bit6 != 0);
    cpu.regs.set_flag(StatusFlags::OVERFLOW, bit6 ^ bit5 != 0);
}

// AXS/SBX: X = (A & X) - #imm (no borrow). Sets flags like CMP.
pub fn axs(cpu: &mut Cpu, bus: &Bus) {
    let addr = cpu.addr_immediate(bus);
    let val = bus.cpu_peek(addr);
    let ax = cpu.regs.a & cpu.regs.x;
    let result = ax.wrapping_sub(val);
    cpu.regs.x = result;
    cpu.regs.set_flag(StatusFlags::CARRY, ax >= val);
    cpu.regs.set_zn(result);
}

// SBC duplicate at 0xEB:identical to official SBC #imm.
pub fn sbc_unofficial(cpu: &mut Cpu, bus: &Bus) {
    let addr = cpu.addr_immediate(bus);
    let val = bus.cpu_peek(addr);
    cpu.sbc(val);
}

// 1-byte NOP (implied). Used by 0x1A, 0x3A, 0x5A, 0x7A, 0xDA, 0xFA.
pub fn nop_implied(_cpu: &mut Cpu, _bus: &mut Bus) {}

// 2-byte NOP (zero page). Reads and discards. Used by 0x04, 0x44, 0x64.
pub fn nop_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.addr_zero_page(bus);
    let _ = bus.cpu_read(addr);
}

// 2-byte NOP (zero page, X). Reads and discards. Used by 0x14, 0x34, 0x54, 0x74, 0xD4, 0xF4.
pub fn nop_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.addr_zero_page_x(bus);
    let _ = bus.cpu_read(addr);
}

// 3-byte NOP (absolute). Reads and discards. Used by 0x0C.
pub fn nop_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.addr_absolute(bus);
    let _ = bus.cpu_read(addr);
}

// 3-byte NOP (absolute, X). Reads and discards. Returns page-cross penalty.
// Used by 0x1C, 0x3C, 0x5C, 0x7C, 0xDC, 0xFC.
pub fn nop_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (addr, crossed) = cpu.addr_absolute_x(bus);
    let _ = bus.cpu_read(addr);
    page_cross_penalty(crossed)
}

// KIL/JAM: freeze the CPU. Used by various undocumented halt opcodes.
pub fn kil(cpu: &mut Cpu, _bus: &mut Bus) {
    log::warn!(
        "KIL/JAM opcode executed at PC={:04X}:CPU halted",
        cpu.pc.wrapping_sub(1)
    );
    cpu.state = crate::hardware::cpu::CpuState::Halted;
}
