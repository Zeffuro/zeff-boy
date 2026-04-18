use crate::hardware::bus::{Bus, OamCorruptionType};
use crate::hardware::cpu::Cpu;
use crate::hardware::types::constants as memory_constants;

macro_rules! ld_rr {
    ($name:ident, $dst:ident, $src:ident) => {
        pub fn $name(cpu: &mut Cpu, _bus: &mut Bus) {
            cpu.regs.$dst = cpu.regs.$src;
        }
    };
}

macro_rules! ld_r_hl {
    ($name:ident, $dst:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            cpu.regs.$dst = cpu.bus_read_timed(bus, cpu.get_hl());
        }
    };
}

macro_rules! ld_hl_r {
    ($name:ident, $src:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            cpu.bus_write_timed(bus, cpu.get_hl(), cpu.regs.$src);
        }
    };
}

macro_rules! ld_r_d8 {
    ($name:ident, $dst:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            cpu.regs.$dst = cpu.fetch8_timed(bus);
        }
    };
}

macro_rules! ld_rp_d16 {
    ($name:ident, $set:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            let value = cpu.fetch16_timed(bus);
            cpu.$set(value);
        }
    };
}

macro_rules! pop_rp {
    ($name:ident, $set:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            let value = cpu.pop16_timed_oam(bus);
            cpu.$set(value);
        }
    };
}

macro_rules! push_rp {
    ($name:ident, $get:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            cpu.tick_internal_timed(bus, 4);
            cpu.push16_timed_oam(bus, cpu.$get());
        }
    };
}

ld_rp_d16!(ld_bc_d16, set_bc);

pub fn ld_bc_a(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_bc(), cpu.regs.a);
}

ld_r_d8!(ld_b_d8, b);

pub fn ld_a16_sp(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.bus_write_timed(bus, addr, (cpu.sp & 0xFF) as u8);
    cpu.bus_write_timed(bus, addr.wrapping_add(1), (cpu.sp >> 8) as u8);
}

pub fn ld_a_bc(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.a = cpu.bus_read_timed(bus, cpu.get_bc());
}

ld_r_d8!(ld_c_d8, c);
ld_rp_d16!(ld_de_d16, set_de);

pub fn ld_de_a(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.bus_write_timed(bus, cpu.get_de(), cpu.regs.a);
}

ld_r_d8!(ld_d_d8, d);

pub fn ld_a_de(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_de();
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}

ld_r_d8!(ld_e_d8, e);
ld_rp_d16!(ld_hl_d16, set_hl);

pub fn ld_hl_plus_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
    cpu.set_hl(addr.wrapping_add(1));
}

ld_r_d8!(ld_h_d8, h);

pub fn ld_a_hl_plus(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
    bus.maybe_trigger_oam_corruption(addr, OamCorruptionType::Read);
    cpu.set_hl(addr.wrapping_add(1));
}

ld_r_d8!(ld_l_d8, l);

pub fn ld_sp_d16(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.sp = cpu.fetch16_timed(bus);
}

pub fn ld_hl_minus_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
    cpu.set_hl(addr.wrapping_sub(1));
}

pub fn ld_hl_d8(cpu: &mut Cpu, bus: &mut Bus) {
    let val = cpu.fetch8_timed(bus);
    cpu.bus_write_timed(bus, cpu.get_hl(), val);
}

pub fn ld_a_hl_minus(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
    bus.maybe_trigger_oam_corruption(addr, OamCorruptionType::Read);
    cpu.set_hl(addr.wrapping_sub(1));
}

ld_r_d8!(ld_a_d8, a);

pub fn ld_b_b(_bus: &mut Bus) {}
ld_rr!(ld_b_c, b, c);
ld_rr!(ld_b_d, b, d);
ld_rr!(ld_b_e, b, e);
ld_rr!(ld_b_h, b, h);
ld_rr!(ld_b_l, b, l);
ld_r_hl!(ld_b_hl, b);
ld_rr!(ld_b_a, b, a);

ld_rr!(ld_c_b, c, b);
pub fn ld_c_c(_bus: &mut Bus) {}
ld_rr!(ld_c_d, c, d);
ld_rr!(ld_c_e, c, e);
ld_rr!(ld_c_h, c, h);
ld_rr!(ld_c_l, c, l);
ld_r_hl!(ld_c_hl, c);
ld_rr!(ld_c_a, c, a);

ld_rr!(ld_d_b, d, b);
ld_rr!(ld_d_c, d, c);
pub fn ld_d_d(_cpu: &mut Cpu, _bus: &mut Bus) {}
ld_rr!(ld_d_e, d, e);
ld_rr!(ld_d_h, d, h);
ld_rr!(ld_d_l, d, l);
ld_r_hl!(ld_d_hl, d);
ld_rr!(ld_d_a, d, a);

ld_rr!(ld_e_b, e, b);
ld_rr!(ld_e_c, e, c);
ld_rr!(ld_e_d, e, d);
pub fn ld_e_e(_bus: &mut Bus) {}
ld_rr!(ld_e_h, e, h);
ld_rr!(ld_e_l, e, l);
ld_r_hl!(ld_e_hl, e);
ld_rr!(ld_e_a, e, a);

ld_rr!(ld_h_b, h, b);
ld_rr!(ld_h_c, h, c);
ld_rr!(ld_h_d, h, d);
ld_rr!(ld_h_e, h, e);
pub fn ld_h_h(_bus: &mut Bus) {}
ld_rr!(ld_h_l, h, l);
ld_r_hl!(ld_h_hl, h);
ld_rr!(ld_h_a, h, a);

ld_rr!(ld_l_b, l, b);
ld_rr!(ld_l_c, l, c);
ld_rr!(ld_l_d, l, d);
ld_rr!(ld_l_e, l, e);
ld_rr!(ld_l_h, l, h);
pub fn ld_l_l(_cpu: &mut Cpu, _bus: &mut Bus) {}
ld_r_hl!(ld_l_hl, l);
ld_rr!(ld_l_a, l, a);

ld_hl_r!(ld_hl_b, b);
ld_hl_r!(ld_hl_c, c);
ld_hl_r!(ld_hl_d, d);
ld_hl_r!(ld_hl_e, e);
ld_hl_r!(ld_hl_h, h);
ld_hl_r!(ld_hl_l, l);
ld_hl_r!(ld_hl_a, a);

ld_rr!(ld_a_b, a, b);
ld_rr!(ld_a_c, a, c);
ld_rr!(ld_a_d, a, d);
ld_rr!(ld_a_e, a, e);
ld_rr!(ld_a_h, a, h);
ld_rr!(ld_a_l, a, l);
ld_r_hl!(ld_a_hl, a);
pub fn ld_a_a(_cpu: &mut Cpu, _: &mut Bus) {}

pop_rp!(pop_bc, set_bc);
push_rp!(push_bc, get_bc);
pop_rp!(pop_de, set_de);
push_rp!(push_de, get_de);

pub fn ldh_a8_a(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus);
    let addr = memory_constants::IO_START | (offset as u16);
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
}

pop_rp!(pop_hl, set_hl);

pub fn ld_c_addr_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = memory_constants::IO_START | (cpu.regs.c as u16);
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
}

push_rp!(push_hl, get_hl);

pub fn ld_a16_a(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.bus_write_timed(bus, addr, cpu.regs.a);
}

pub fn ldh_a_a8(cpu: &mut Cpu, bus: &mut Bus) {
    let offset = cpu.fetch8_timed(bus);
    let addr = memory_constants::IO_START | (offset as u16);
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}

pop_rp!(pop_af, set_af);

pub fn ld_a_c_addr(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = memory_constants::IO_START | (cpu.regs.c as u16);
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}

push_rp!(push_af, get_af);

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

pub fn ld_sp_hl(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    cpu.sp = cpu.get_hl();
}

pub fn ld_a_a16(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.fetch16_timed(bus);
    cpu.regs.a = cpu.bus_read_timed(bus, addr);
}
