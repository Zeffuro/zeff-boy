use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::cpu::registers::StatusFlags;

#[inline(always)]
fn page_cross_penalty(crossed: bool) -> u8 {
    crossed as u8
}

// 0xA9: LDA #imm
pub fn lda_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0xA5: LDA zp
pub fn lda_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0xB5: LDA zp,X
pub fn lda_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0xAD: LDA abs
pub fn lda_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0xBD: LDA abs,X:+1 cycle on page cross
pub fn lda_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0xB9: LDA abs,Y:+1 cycle on page cross
pub fn lda_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0xA1: LDA (ind,X)
pub fn lda_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0xB1: LDA (ind),Y:+1 cycle on page cross
pub fn lda_ind_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_indirect_y(bus);
    cpu.regs.a = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0xA2: LDX #imm
pub fn ldx_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    cpu.regs.x = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.x);
}

// 0xA6: LDX zp
pub fn ldx_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.regs.x = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.x);
}

// 0xB6: LDX zp,Y
pub fn ldx_zp_y(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_y(bus);
    cpu.regs.x = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.x);
}

// 0xAE: LDX abs
pub fn ldx_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.regs.x = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.x);
}

// 0xBE: LDX abs,Y:+1 cycle on page cross
pub fn ldx_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    cpu.regs.x = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.x);
    page_cross_penalty(crossed)
}

// 0xA0: LDY #imm
pub fn ldy_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    cpu.regs.y = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.y);
}

// 0xA4: LDY zp
pub fn ldy_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.regs.y = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.y);
}

// 0xB4: LDY zp,X
pub fn ldy_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    cpu.regs.y = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.y);
}

// 0xAC: LDY abs
pub fn ldy_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.regs.y = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.y);
}

// 0xBC: LDY abs,X:+1 cycle on page cross
pub fn ldy_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    cpu.regs.y = bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.y);
    page_cross_penalty(crossed)
}

// 0x85: STA zp
pub fn sta_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    bus.cpu_write(a, cpu.regs.a);
}

// 0x95: STA zp,X
pub fn sta_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    bus.cpu_write(a, cpu.regs.a);
}

// 0x8D: STA abs
pub fn sta_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    bus.cpu_write(a, cpu.regs.a);
}

// 0x9D: STA abs,X
pub fn sta_abs_x(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_x(bus);
    bus.cpu_write(a, cpu.regs.a);
}

// 0x99: STA abs,Y
pub fn sta_abs_y(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_y(bus);
    bus.cpu_write(a, cpu.regs.a);
}

// 0x81: STA (ind,X)
pub fn sta_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    bus.cpu_write(a, cpu.regs.a);
}

// 0x91: STA (ind),Y
pub fn sta_ind_y(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_indirect_y(bus);
    bus.cpu_write(a, cpu.regs.a);
}

// 0x86: STX zp
pub fn stx_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    bus.cpu_write(a, cpu.regs.x);
}

// 0x96: STX zp,Y
pub fn stx_zp_y(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_y(bus);
    bus.cpu_write(a, cpu.regs.x);
}

// 0x8E: STX abs
pub fn stx_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    bus.cpu_write(a, cpu.regs.x);
}

// 0x84: STY zp
pub fn sty_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    bus.cpu_write(a, cpu.regs.y);
}

// 0x94: STY zp,X
pub fn sty_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    bus.cpu_write(a, cpu.regs.y);
}

// 0x8C: STY abs
pub fn sty_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    bus.cpu_write(a, cpu.regs.y);
}

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
