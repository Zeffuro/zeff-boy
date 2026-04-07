use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

macro_rules! alu_reg {
    ($name:ident, $method:ident, $reg:ident) => {
        pub fn $name(cpu: &mut Cpu, _bus: &mut Bus) {
            cpu.$method(cpu.regs.$reg);
        }
    };
}

macro_rules! alu_hl_mem {
    ($name:ident, $method:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            let value = cpu.bus_read_timed(bus, cpu.get_hl());
            cpu.$method(value);
        }
    };
}

macro_rules! alu_d8 {
    ($name:ident, $method:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            let value = cpu.fetch8_timed(bus);
            cpu.$method(value);
        }
    };
}

macro_rules! inc8 {
    ($name:ident, $reg:ident) => {
        pub fn $name(cpu: &mut Cpu, _bus: &mut Bus) {
            cpu.regs.$reg = cpu.inc(cpu.regs.$reg);
        }
    };
}

macro_rules! dec8 {
    ($name:ident, $reg:ident) => {
        pub fn $name(cpu: &mut Cpu, _bus: &mut Bus) {
            cpu.regs.$reg = cpu.dec(cpu.regs.$reg);
        }
    };
}

macro_rules! inc16 {
    ($name:ident, $get:ident, $set:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            let value = cpu.inc_rp_timed(bus, cpu.$get());
            cpu.$set(value);
        }
    };
}

macro_rules! dec16 {
    ($name:ident, $get:ident, $set:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            let value = cpu.dec_rp_timed(bus, cpu.$get());
            cpu.$set(value);
        }
    };
}

macro_rules! add_hl_rp {
    ($name:ident, $get:ident) => {
        pub fn $name(cpu: &mut Cpu, bus: &mut Bus) {
            cpu.tick_internal_timed(bus, 4);
            let hl = cpu.get_hl();
            let rp = cpu.$get();
            let result = hl.wrapping_add(rp);
            cpu.set_n(false);
            cpu.set_h((hl & 0x0FFF) + (rp & 0x0FFF) > 0x0FFF);
            cpu.set_c((hl as u32) + (rp as u32) > 0xFFFF);
            cpu.set_hl(result);
        }
    };
}

inc16!(inc_bc, get_bc, set_bc);
inc8!(inc_b, b);
dec8!(dec_b, b);
add_hl_rp!(add_hl_bc, get_bc);
dec16!(dec_bc, get_bc, set_bc);
inc8!(inc_c, c);
dec8!(dec_c, c);

inc16!(inc_de, get_de, set_de);
inc8!(inc_d, d);
dec8!(dec_d, d);
add_hl_rp!(add_hl_de, get_de);
dec16!(dec_de, get_de, set_de);
inc8!(inc_e, e);
dec8!(dec_e, e);

inc16!(inc_hl, get_hl, set_hl);
inc8!(inc_h, h);
dec8!(dec_h, h);

pub fn daa(cpu: &mut Cpu, _bus: &mut Bus) {
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

add_hl_rp!(add_hl_hl, get_hl);
dec16!(dec_hl, get_hl, set_hl);
inc8!(inc_l, l);
dec8!(dec_l, l);

pub fn cpl(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = !cpu.regs.a;
    cpu.set_n(true);
    cpu.set_h(true);
}

pub fn inc_sp(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.sp = cpu.inc_rp_timed(bus, cpu.sp);
}

pub fn inc_hl_val(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = cpu.bus_read_timed(bus, addr);
    let new_val = cpu.inc(val);
    cpu.bus_write_timed(bus, addr, new_val);
}

pub fn dec_hl_val(cpu: &mut Cpu, bus: &mut Bus) {
    let addr = cpu.get_hl();
    let val = cpu.bus_read_timed(bus, addr);
    let new_val = cpu.dec(val);
    cpu.bus_write_timed(bus, addr, new_val);
}

pub fn scf(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.set_n(false);
    cpu.set_h(false);
    cpu.set_c(true);
}

pub fn add_hl_sp(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.tick_internal_timed(bus, 4);
    let hl = cpu.get_hl();
    let sp = cpu.sp;
    let result = hl.wrapping_add(sp);
    cpu.set_n(false);
    cpu.set_h((hl & 0x0FFF) + (sp & 0x0FFF) > 0x0FFF);
    cpu.set_c((hl as u32) + (sp as u32) > 0xFFFF);
    cpu.set_hl(result);
}

pub fn dec_sp(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.sp = cpu.dec_rp_timed(bus, cpu.sp);
}

inc8!(inc_a, a);
dec8!(dec_a, a);

pub fn ccf(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.set_n(false);
    cpu.set_h(false);
    cpu.set_c(!cpu.get_c());
}

alu_reg!(add_b, add, b);
alu_reg!(add_c, add, c);
alu_reg!(add_d, add, d);
alu_reg!(add_e, add, e);
alu_reg!(add_h, add, h);
alu_reg!(add_l, add, l);
alu_hl_mem!(add_hl, add);
alu_reg!(add_a, add, a);

alu_reg!(adc_a_b, adc, b);
alu_reg!(adc_a_c, adc, c);
alu_reg!(adc_a_d, adc, d);
alu_reg!(adc_a_e, adc, e);
alu_reg!(adc_a_h, adc, h);
alu_reg!(adc_a_l, adc, l);
alu_hl_mem!(adc_a_hl, adc);
alu_reg!(adc_a_a, adc, a);

alu_reg!(sub_b, sub, b);
alu_reg!(sub_c, sub, c);
alu_reg!(sub_d, sub, d);
alu_reg!(sub_e, sub, e);
alu_reg!(sub_h, sub, h);
alu_reg!(sub_l, sub, l);
alu_hl_mem!(sub_hl, sub);
alu_reg!(sub_a, sub, a);

alu_reg!(sbc_a_b, sbc, b);
alu_reg!(sbc_a_c, sbc, c);
alu_reg!(sbc_a_d, sbc, d);
alu_reg!(sbc_a_e, sbc, e);
alu_reg!(sbc_a_h, sbc, h);
alu_reg!(sbc_a_l, sbc, l);
alu_hl_mem!(sbc_a_hl, sbc);
alu_reg!(sbc_a_a, sbc, a);

alu_reg!(and_b, logical_and, b);
alu_reg!(and_c, logical_and, c);
alu_reg!(and_d, logical_and, d);
alu_reg!(and_e, logical_and, e);
alu_reg!(and_h, logical_and, h);
alu_reg!(and_l, logical_and, l);
alu_hl_mem!(and_hl, logical_and);
alu_reg!(and_a, logical_and, a);

alu_reg!(xor_b, logical_xor, b);
alu_reg!(xor_c, logical_xor, c);
alu_reg!(xor_d, logical_xor, d);
alu_reg!(xor_e, logical_xor, e);
alu_reg!(xor_h, logical_xor, h);
alu_reg!(xor_l, logical_xor, l);
alu_hl_mem!(xor_hl, logical_xor);
alu_reg!(xor_a, logical_xor, a);

alu_reg!(or_b, logical_or, b);
alu_reg!(or_c, logical_or, c);
alu_reg!(or_d, logical_or, d);
alu_reg!(or_e, logical_or, e);
alu_reg!(or_h, logical_or, h);
alu_reg!(or_l, logical_or, l);
alu_hl_mem!(or_hl, logical_or);
alu_reg!(or_a, logical_or, a);

alu_reg!(cp_b, compare, b);
alu_reg!(cp_c, compare, c);
alu_reg!(cp_d, compare, d);
alu_reg!(cp_e, compare, e);
alu_reg!(cp_h, compare, h);
alu_reg!(cp_l, compare, l);
alu_hl_mem!(cp_hl, compare);
alu_reg!(cp_a, compare, a);

alu_d8!(add_a_d8, add);
alu_d8!(adc_a_d8, adc);
alu_d8!(sub_d8, sub);
alu_d8!(sbc_a_d8, sbc);
alu_d8!(and_d8, logical_and);

pub fn add_sp_r8(cpu: &mut Cpu, bus: &mut Bus) {
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

alu_d8!(xor_d8, logical_xor);
alu_d8!(or_d8, logical_or);
alu_d8!(cp_d8, compare);
