use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::cpu::registers::StatusFlags;

#[inline(always)]
fn page_cross_penalty(crossed: bool) -> u8 {
    crossed as u8
}

macro_rules! load_all_modes {
    ($reg:ident, $imm:ident, $zp:ident, $zpx:ident, $abs:ident,
     $absx:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $imm(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_immediate(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page_x(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_x(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
            page_cross_penalty(crossed)
        }
        pub fn $absy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_absolute_y(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
            page_cross_penalty(crossed)
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_indirect_x(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $indy(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.addr_indirect_y(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
            page_cross_penalty(crossed)
        }
    };
    ($reg:ident, $imm:ident, $zp:ident, $zpalt:ident, $abs:ident, $absalt:ident;
     zp_addr = $zp_addr:ident, abs_addr = $abs_addr:ident) => {
        pub fn $imm(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_immediate(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $zpalt(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.$zp_addr(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
        }
        pub fn $absalt(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
            let (a, crossed) = cpu.$abs_addr(bus);
            cpu.regs.$reg = bus.cpu_read(a);
            cpu.regs.set_zn(cpu.regs.$reg);
            page_cross_penalty(crossed)
        }
    };
}

macro_rules! store_all_modes {
    ($reg:ident, $zp:ident, $zpx:ident, $abs:ident,
     $absx:ident, $absy:ident, $indx:ident, $indy:ident) => {
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $zpx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page_x(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $absx(cpu: &mut Cpu, bus: &mut Bus) {
            let (a, _) = cpu.addr_absolute_x(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $absy(cpu: &mut Cpu, bus: &mut Bus) {
            let (a, _) = cpu.addr_absolute_y(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $indx(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_indirect_x(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $indy(cpu: &mut Cpu, bus: &mut Bus) {
            let (a, _) = cpu.addr_indirect_y(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
    };
    ($reg:ident, $zp:ident, $zpalt:ident, $abs:ident; alt = $alt_addr:ident) => {
        pub fn $zp(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_zero_page(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $zpalt(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.$alt_addr(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
        pub fn $abs(cpu: &mut Cpu, bus: &mut Bus) {
            let a = cpu.addr_absolute(bus);
            bus.cpu_write(a, cpu.regs.$reg);
        }
    };
}

// LDA: 0xA9, 0xA5, 0xB5, 0xAD, 0xBD, 0xB9, 0xA1, 0xB1
load_all_modes!(
    a, lda_imm, lda_zp, lda_zp_x, lda_abs, lda_abs_x, lda_abs_y, lda_ind_x, lda_ind_y
);

// LDX: 0xA2, 0xA6, 0xB6, 0xAE, 0xBE
load_all_modes!(x, ldx_imm, ldx_zp, ldx_zp_y, ldx_abs, ldx_abs_y;
    zp_addr = addr_zero_page_y, abs_addr = addr_absolute_y);

// LDY: 0xA0, 0xA4, 0xB4, 0xAC, 0xBC
load_all_modes!(y, ldy_imm, ldy_zp, ldy_zp_x, ldy_abs, ldy_abs_x;
    zp_addr = addr_zero_page_x, abs_addr = addr_absolute_x);

// STA: 0x85, 0x95, 0x8D, 0x9D, 0x99, 0x81, 0x91
store_all_modes!(
    a, sta_zp, sta_zp_x, sta_abs, sta_abs_x, sta_abs_y, sta_ind_x, sta_ind_y
);

// STX: 0x86, 0x96, 0x8E
store_all_modes!(x, stx_zp, stx_zp_y, stx_abs; alt = addr_zero_page_y);

// STY: 0x84, 0x94, 0x8C
store_all_modes!(y, sty_zp, sty_zp_x, sty_abs; alt = addr_zero_page_x);

// 0xAA: TAX
pub fn tax(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.x = cpu.regs.a;
    cpu.regs.set_zn(cpu.regs.x);
}

// 0xA8: TAY
pub fn tay(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.y = cpu.regs.a;
    cpu.regs.set_zn(cpu.regs.y);
}

// 0xBA: TSX
pub fn tsx(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.x = cpu.sp;
    cpu.regs.set_zn(cpu.regs.x);
}

// 0x8A: TXA
pub fn txa(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.x;
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x9A: TXS
pub fn txs(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.sp = cpu.regs.x;
}

// 0x98: TYA
pub fn tya(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.a = cpu.regs.y;
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x48: PHA
pub fn pha(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.push8(bus, cpu.regs.a);
}

// 0x08: PHP
pub fn php(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.push8(bus, cpu.regs.status_for_push(true));
}

// 0x68: PLA
pub fn pla(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.regs.a = cpu.pop8(bus);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x28: PLP
pub fn plp(cpu: &mut Cpu, bus: &mut Bus) {
    let v = cpu.pop8(bus);
    cpu.regs.p = StatusFlags::from_bits_truncate((v & 0xEF) | 0x20);
}
