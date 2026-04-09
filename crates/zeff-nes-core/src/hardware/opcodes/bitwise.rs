use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

#[inline(always)]
fn page_cross_penalty(crossed: bool) -> u8 {
    crossed as u8
}

macro_rules! bitwise_all_modes {
    ($bitop:tt, $imm:ident, $zp:ident, $zpx:ident, $abs:ident,
     $absx:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $imm(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_immediate(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
        }
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page_x(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_x(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
            page_cross_penalty(crossed)
        }
        pub fn $absy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_y(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
            page_cross_penalty(crossed)
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_indirect_x(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
        }
        pub fn $indy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_indirect_y(bus);
            cpu.regs.a $bitop bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.a);
            page_cross_penalty(crossed)
        }
    };
}

macro_rules! rmw_shift_modes {
    ($acc_fn:ident, $val_fn:ident, $acc:ident, $zp:ident, $zpx:ident, $abs:ident, $absx:ident) => {
        pub fn $acc(cpu: &mut Cpu, _bus: &mut Bus) {
            cpu.$acc_fn();
        }
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            let v = cpu.$val_fn(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page_x(bus);
            let v = cpu.$val_fn(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            let v = cpu.$val_fn(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) {
            let (a, _) = cpu.addr_absolute_x(bus);
            let v = cpu.$val_fn(bus.cpu_read(a));
            bus.cpu_write(a, v);
        }
    };
}

// AND: 0x29, 0x25, 0x35, 0x2D, 0x3D, 0x39, 0x21, 0x31
bitwise_all_modes!(&=, and_imm, and_zp, and_zp_x, and_abs,
    and_abs_x, and_abs_y, and_ind_x, and_ind_y);

// ORA: 0x09, 0x05, 0x15, 0x0D, 0x1D, 0x19, 0x01, 0x11
bitwise_all_modes!(|=, ora_imm, ora_zp, ora_zp_x, ora_abs,
    ora_abs_x, ora_abs_y, ora_ind_x, ora_ind_y);

// EOR: 0x49, 0x45, 0x55, 0x4D, 0x5D, 0x59, 0x41, 0x51
bitwise_all_modes!(^=, eor_imm, eor_zp, eor_zp_x, eor_abs,
    eor_abs_x, eor_abs_y, eor_ind_x, eor_ind_y);

// BIT: 0x24, 0x2C
pub fn bit_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.bit_test(bus.cpu_read(a));
}

pub fn bit_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.bit_test(bus.cpu_read(a));
}

// ASL: 0x0A, 0x06, 0x16, 0x0E, 0x1E
rmw_shift_modes!(asl_acc, asl_val, asl_acc, asl_zp, asl_zp_x, asl_abs, asl_abs_x);

// LSR: 0x4A, 0x46, 0x56, 0x4E, 0x5E
rmw_shift_modes!(lsr_acc, lsr_val, lsr_acc, lsr_zp, lsr_zp_x, lsr_abs, lsr_abs_x);

// ROL: 0x2A, 0x26, 0x36, 0x2E, 0x3E
rmw_shift_modes!(rol_acc, rol_val, rol_acc, rol_zp, rol_zp_x, rol_abs, rol_abs_x);

// ROR: 0x6A, 0x66, 0x76, 0x6E, 0x7E
rmw_shift_modes!(ror_acc, ror_val, ror_acc, ror_zp, ror_zp_x, ror_abs, ror_abs_x);
