use crate::hardware::cpu::registers::StatusFlags;
use crate::hardware::cpu::{Cpu, CpuBus};

const UNSTABLE_IMMEDIATE_MASK: u8 = 0xEE;

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
        pub fn $zp<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_zero_page(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $zpy<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_zero_page_y(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $abs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_absolute(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $absy<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
            let (addr, crossed) = cpu.addr_absolute_y_read(bus);
            lax_set(cpu, bus.cpu_read(addr));
            page_cross_penalty(crossed)
        }
        pub fn $indx<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_indirect_x(bus);
            lax_set(cpu, bus.cpu_read(addr));
        }
        pub fn $indy<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
            let (addr, crossed) = cpu.addr_indirect_y_read(bus);
            lax_set(cpu, bus.cpu_read(addr));
            page_cross_penalty(crossed)
        }
    };
}

lax_modes!(lax_zp, lax_zp_y, lax_abs, lax_abs_y, lax_ind_x, lax_ind_y);

// LAS/LAR/LAE: load A, X, and SP with memory & SP. The absolute,Y mode has
// the same page-cross timing behavior as normal indexed loads.
pub fn las_abs_y<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    let (addr, crossed) = cpu.addr_absolute_y_read(bus);
    let val = bus.cpu_read(addr) & cpu.sp;
    cpu.regs.a = val;
    cpu.regs.x = val;
    cpu.sp = val;
    cpu.regs.set_zn(val);
    page_cross_penalty(crossed)
}

// ── SAX: store A & X ────────────────────────────────────────────────

macro_rules! sax_modes {
    ($zp:ident, $zpy:ident, $abs:ident, $indx:ident) => {
        pub fn $zp<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_zero_page(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
        pub fn $zpy<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_zero_page_y(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
        pub fn $abs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_absolute(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
        pub fn $indx<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_indirect_x(bus);
            bus.cpu_write(addr, cpu.regs.a & cpu.regs.x);
        }
    };
}

sax_modes!(sax_zp, sax_zp_y, sax_abs, sax_ind_x);

#[inline]
fn unstable_high_byte_mask(base: u16) -> u8 {
    ((base >> 8) as u8).wrapping_add(1)
}

fn unstable_store_write_addr(base: u16, indexed: u16, value: u8) -> u16 {
    if (base & 0xFF00) == (indexed & 0xFF00) {
        indexed
    } else {
        (u16::from(value) << 8) | (indexed & 0x00FF)
    }
}

fn addr_absolute_x_unstable_store<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> (u16, u16) {
    let base = cpu.addr_absolute(bus);
    let indexed = base.wrapping_add(u16::from(cpu.regs.x));
    let dummy_addr = (base & 0xFF00) | (indexed & 0x00FF);
    let _ = bus.cpu_read_after_elapsed_cycles(dummy_addr, 3);
    (base, indexed)
}

fn addr_absolute_y_unstable_store<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> (u16, u16) {
    let base = cpu.addr_absolute(bus);
    let indexed = base.wrapping_add(u16::from(cpu.regs.y));
    let dummy_addr = (base & 0xFF00) | (indexed & 0x00FF);
    let _ = bus.cpu_read_after_elapsed_cycles(dummy_addr, 3);
    (base, indexed)
}

fn addr_indirect_y_unstable_store<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> (u16, u16) {
    let zp = cpu.fetch8(bus);
    let lo = u16::from(bus.cpu_read(u16::from(zp)));
    let hi = u16::from(bus.cpu_read(u16::from(zp.wrapping_add(1))));
    let base = (hi << 8) | lo;
    let indexed = base.wrapping_add(u16::from(cpu.regs.y));
    let dummy_addr = (base & 0xFF00) | (indexed & 0x00FF);
    let _ = bus.cpu_read_after_elapsed_cycles(dummy_addr, 4);
    (base, indexed)
}

pub fn ahx_ind_y<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let (base, indexed) = addr_indirect_y_unstable_store(cpu, bus);
    let val = cpu.regs.a & cpu.regs.x & unstable_high_byte_mask(base);
    bus.cpu_write_after_elapsed_cycles(unstable_store_write_addr(base, indexed, val), val, 5);
}

pub fn tas_abs_y<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let (base, indexed) = addr_absolute_y_unstable_store(cpu, bus);
    cpu.sp = cpu.regs.a & cpu.regs.x;
    let val = cpu.sp & unstable_high_byte_mask(base);
    bus.cpu_write_after_elapsed_cycles(unstable_store_write_addr(base, indexed, val), val, 4);
}

pub fn shy_abs_x<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let (base, indexed) = addr_absolute_x_unstable_store(cpu, bus);
    let val = cpu.regs.y & unstable_high_byte_mask(base);
    bus.cpu_write_after_elapsed_cycles(unstable_store_write_addr(base, indexed, val), val, 4);
}

pub fn shx_abs_y<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let (base, indexed) = addr_absolute_y_unstable_store(cpu, bus);
    let val = cpu.regs.x & unstable_high_byte_mask(base);
    bus.cpu_write_after_elapsed_cycles(unstable_store_write_addr(base, indexed, val), val, 4);
}

pub fn ahx_abs_y<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let (base, indexed) = addr_absolute_y_unstable_store(cpu, bus);
    let val = cpu.regs.a & cpu.regs.x & unstable_high_byte_mask(base);
    bus.cpu_write_after_elapsed_cycles(unstable_store_write_addr(base, indexed, val), val, 4);
}

// ── DCP: DEC + CMP ─────────────────────────────────────────────────

fn dcp_op<B: CpuBus>(cpu: &mut Cpu, bus: &mut B, addr: u16) {
    let old = bus.cpu_read(addr);
    bus.cpu_write(addr, old);
    let val = old.wrapping_sub(1);
    bus.cpu_write(addr, val);
    cpu.compare(cpu.regs.a, val);
}

// ── ISB (ISC): INC + SBC ───────────────────────────────────────────

fn isb_op<B: CpuBus>(cpu: &mut Cpu, bus: &mut B, addr: u16) {
    let old = bus.cpu_read(addr);
    bus.cpu_write(addr, old);
    let val = old.wrapping_add(1);
    bus.cpu_write(addr, val);
    cpu.sbc(val);
}

// ── SLO: ASL + ORA ─────────────────────────────────────────────────

fn slo_op<B: CpuBus>(cpu: &mut Cpu, bus: &mut B, addr: u16) {
    let old = bus.cpu_read(addr);
    bus.cpu_write(addr, old);
    let shifted = cpu.asl_val(old);
    bus.cpu_write(addr, shifted);
    cpu.regs.a |= shifted;
    cpu.regs.set_zn(cpu.regs.a);
}

// ── RLA: ROL + AND ─────────────────────────────────────────────────

fn rla_op<B: CpuBus>(cpu: &mut Cpu, bus: &mut B, addr: u16) {
    let old = bus.cpu_read(addr);
    bus.cpu_write(addr, old);
    let rotated = cpu.rol_val(old);
    bus.cpu_write(addr, rotated);
    cpu.regs.a &= rotated;
    cpu.regs.set_zn(cpu.regs.a);
}

// ── SRE: LSR + EOR ─────────────────────────────────────────────────

fn sre_op<B: CpuBus>(cpu: &mut Cpu, bus: &mut B, addr: u16) {
    let old = bus.cpu_read(addr);
    bus.cpu_write(addr, old);
    let shifted = cpu.lsr_val(old);
    bus.cpu_write(addr, shifted);
    cpu.regs.a ^= shifted;
    cpu.regs.set_zn(cpu.regs.a);
}

// ── RRA: ROR + ADC ─────────────────────────────────────────────────

fn rra_op<B: CpuBus>(cpu: &mut Cpu, bus: &mut B, addr: u16) {
    let old = bus.cpu_read(addr);
    bus.cpu_write(addr, old);
    let rotated = cpu.ror_val(old);
    bus.cpu_write(addr, rotated);
    cpu.adc(rotated);
}

macro_rules! rmw_unofficial_modes {
    ($op:ident, $zp:ident, $zpx:ident, $abs:ident,
     $absx:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $zp<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_zero_page(bus);
            $op(cpu, bus, addr);
        }
        pub fn $zpx<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_zero_page_x(bus);
            $op(cpu, bus, addr);
        }
        pub fn $abs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_absolute(bus);
            $op(cpu, bus, addr);
        }
        pub fn $absx<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_absolute_x_write(bus);
            $op(cpu, bus, addr);
        }
        pub fn $absy<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_absolute_y_write(bus);
            $op(cpu, bus, addr);
        }
        pub fn $indx<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_indirect_x(bus);
            $op(cpu, bus, addr);
        }
        pub fn $indy<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
            let addr = cpu.addr_indirect_y_write(bus);
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
pub fn anc<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let val = cpu.fetch8(bus);
    cpu.regs.a &= val;
    cpu.regs.set_zn(cpu.regs.a);
    cpu.regs
        .set_flag(StatusFlags::CARRY, cpu.regs.a & 0x80 != 0);
}

// ALR: AND #imm, then LSR A.
pub fn alr<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let val = cpu.fetch8(bus);
    cpu.regs.a &= val;
    cpu.lsr_acc();
}

// ARR: AND #imm, then ROR A. Carry and overflow set specially.
pub fn arr<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let val = cpu.fetch8(bus);
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
pub fn axs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let val = cpu.fetch8(bus);
    let ax = cpu.regs.a & cpu.regs.x;
    let result = ax.wrapping_sub(val);
    cpu.regs.x = result;
    cpu.regs.set_flag(StatusFlags::CARRY, ax >= val);
    cpu.regs.set_zn(result);
}

pub fn ane<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let val = cpu.fetch8(bus);
    cpu.regs.a = (cpu.regs.a | UNSTABLE_IMMEDIATE_MASK) & cpu.regs.x & val;
    cpu.regs.set_zn(cpu.regs.a);
}

pub fn atx<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let val = cpu.fetch8(bus);
    let result = (cpu.regs.a | UNSTABLE_IMMEDIATE_MASK) & val;
    cpu.regs.a = result;
    cpu.regs.x = result;
    cpu.regs.set_zn(result);
}

// SBC duplicate at 0xEB:identical to official SBC #imm.
pub fn sbc_unofficial<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let val = cpu.fetch8(bus);
    cpu.sbc(val);
}

// 1-byte NOP (implied). Used by 0x1A, 0x3A, 0x5A, 0x7A, 0xDA, 0xFA.
pub fn nop_implied<B: CpuBus>(_cpu: &mut Cpu, _bus: &mut B) {}

// 2-byte NOP (zero page). Reads and discards. Used by 0x04, 0x44, 0x64.
pub fn nop_zp<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let addr = cpu.addr_zero_page(bus);
    let _ = bus.cpu_read(addr);
}

// 2-byte NOP (zero page, X). Reads and discards. Used by 0x14, 0x34, 0x54, 0x74, 0xD4, 0xF4.
pub fn nop_zp_x<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let addr = cpu.addr_zero_page_x(bus);
    let _ = bus.cpu_read(addr);
}

// 3-byte NOP (absolute). Reads and discards. Used by 0x0C.
pub fn nop_abs<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) {
    let addr = cpu.addr_absolute(bus);
    let _ = bus.cpu_read(addr);
}

// 3-byte NOP (absolute, X). Reads and discards. Returns page-cross penalty.
// Used by 0x1C, 0x3C, 0x5C, 0x7C, 0xDC, 0xFC.
pub fn nop_abs_x<B: CpuBus>(cpu: &mut Cpu, bus: &mut B) -> u8 {
    let (addr, crossed) = cpu.addr_absolute_x_read(bus);
    let _ = bus.cpu_read(addr);
    page_cross_penalty(crossed)
}

// KIL/JAM: freeze the CPU. Used by various undocumented halt opcodes.
pub fn kil<B: CpuBus>(cpu: &mut Cpu, _bus: &mut B) {
    log::warn!(
        "KIL/JAM opcode executed at PC={:04X}:CPU halted",
        cpu.pc.wrapping_sub(1)
    );
    cpu.enter_jam();
}
