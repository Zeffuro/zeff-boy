use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

#[inline(always)]
fn page_cross_penalty(crossed: bool) -> u8 {
    crossed as u8
}

macro_rules! read_op_all_modes {
    ($op:ident, $imm:ident, $zp:ident, $zpx:ident, $abs:ident,
     $absx:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $imm(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_immediate(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
        }
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page_x(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_x(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
            page_cross_penalty(crossed)
        }
        pub fn $absy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_y(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
            page_cross_penalty(crossed)
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_indirect_x(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
        }
        pub fn $indy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_indirect_y(bus);
            let v = bus.cpu_read(a);
            cpu.$op(v);
            page_cross_penalty(crossed)
        }
    };
}

macro_rules! compare_all_modes {
    ($reg:ident, $imm:ident, $zp:ident, $zpx:ident, $abs:ident,
     $absx:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $imm(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_immediate(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page_x(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_x(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
            page_cross_penalty(crossed)
        }
        pub fn $absy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_y(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
            page_cross_penalty(crossed)
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_indirect_x(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
        pub fn $indy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_indirect_y(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
            page_cross_penalty(crossed)
        }
    };
    ($reg:ident, $imm:ident, $zp:ident, $abs:ident) => {
        pub fn $imm(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_immediate(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            let v = bus.cpu_read(a);
            cpu.compare(cpu.regs.$reg, v);
        }
    };
}

macro_rules! rmw_modes {
    ($op:ident, $zp:ident, $zpx:ident, $abs:ident, $absx:ident) => {
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            let v = cpu.$op(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page_x(bus);
            let v = cpu.$op(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            let v = cpu.$op(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) {
            let (a, _) = cpu.addr_absolute_x(bus);
            let v = cpu.$op(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
    };
}

// ADC: 0x69, 0x65, 0x75, 0x6D, 0x7D, 0x79, 0x61, 0x71
read_op_all_modes!(adc, adc_imm, adc_zp, adc_zp_x, adc_abs,
    adc_abs_x, adc_abs_y, adc_ind_x, adc_ind_y);

// SBC: 0xE9, 0xE5, 0xF5, 0xED, 0xFD, 0xF9, 0xE1, 0xF1
read_op_all_modes!(sbc, sbc_imm, sbc_zp, sbc_zp_x, sbc_abs,
    sbc_abs_x, sbc_abs_y, sbc_ind_x, sbc_ind_y);

// CMP: 0xC9, 0xC5, 0xD5, 0xCD, 0xDD, 0xD9, 0xC1, 0xD1
compare_all_modes!(a, cmp_imm, cmp_zp, cmp_zp_x, cmp_abs,
    cmp_abs_x, cmp_abs_y, cmp_ind_x, cmp_ind_y);

// CPX: 0xE0, 0xE4, 0xEC
compare_all_modes!(x, cpx_imm, cpx_zp, cpx_abs);

// CPY: 0xC0, 0xC4, 0xCC
compare_all_modes!(y, cpy_imm, cpy_zp, cpy_abs);

// INC: 0xE6, 0xF6, 0xEE, 0xFE
rmw_modes!(inc_val, inc_zp, inc_zp_x, inc_abs, inc_abs_x);

// DEC: 0xC6, 0xD6, 0xCE, 0xDE
rmw_modes!(dec_val, dec_zp, dec_zp_x, dec_abs, dec_abs_x);

// 0xE8: INX
pub fn inx(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.x = cpu.regs.x.wrapping_add(1);
    cpu.regs.set_zn(cpu.regs.x);
}

// 0xC8: INY
pub fn iny(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.y = cpu.regs.y.wrapping_add(1);
    cpu.regs.set_zn(cpu.regs.y);
}

// 0xCA: DEX
pub fn dex(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.x = cpu.regs.x.wrapping_sub(1);
    cpu.regs.set_zn(cpu.regs.x);
}

// 0x88: DEY
pub fn dey(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.y = cpu.regs.y.wrapping_sub(1);
    cpu.regs.set_zn(cpu.regs.y);
}
