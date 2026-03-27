use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

#[inline(always)]
fn page_cross_penalty(crossed: bool) -> u8 {
    crossed as u8
}

// 0x69: ADC #imm
pub fn adc_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
}

// 0x65: ADC zp
pub fn adc_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
}

// 0x75: ADC zp,X
pub fn adc_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
}

// 0x6D: ADC abs
pub fn adc_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
}

// 0x7D: ADC abs,X — +1 on page cross
pub fn adc_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
    page_cross_penalty(crossed)
}

// 0x79: ADC abs,Y — +1 on page cross
pub fn adc_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
    page_cross_penalty(crossed)
}

// 0x61: ADC (ind,X)
pub fn adc_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
}

// 0x71: ADC (ind),Y — +1 on page cross
pub fn adc_ind_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_indirect_y(bus);
    let v = bus.cpu_read(a);
    cpu.adc(v);
    page_cross_penalty(crossed)
}

// 0xE9: SBC #imm
pub fn sbc_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
}

// 0xE5: SBC zp
pub fn sbc_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
}

// 0xF5: SBC zp,X
pub fn sbc_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
}

// 0xED: SBC abs
pub fn sbc_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
}

// 0xFD: SBC abs,X — +1 on page cross
pub fn sbc_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
    page_cross_penalty(crossed)
}

// 0xF9: SBC abs,Y — +1 on page cross
pub fn sbc_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
    page_cross_penalty(crossed)
}

// 0xE1: SBC (ind,X)
pub fn sbc_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
}

// 0xF1: SBC (ind),Y — +1 on page cross
pub fn sbc_ind_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_indirect_y(bus);
    let v = bus.cpu_read(a);
    cpu.sbc(v);
    page_cross_penalty(crossed)
}

// 0xC9: CMP #imm
pub fn cmp_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
}

// 0xC5: CMP zp
pub fn cmp_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
}

// 0xD5: CMP zp,X
pub fn cmp_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
}

// 0xCD: CMP abs
pub fn cmp_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
}

// 0xDD: CMP abs,X — +1 on page cross
pub fn cmp_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
    page_cross_penalty(crossed)
}

// 0xD9: CMP abs,Y — +1 on page cross
pub fn cmp_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
    page_cross_penalty(crossed)
}

// 0xC1: CMP (ind,X)
pub fn cmp_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
}

// 0xD1: CMP (ind),Y — +1 on page cross
pub fn cmp_ind_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_indirect_y(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.a, v);
    page_cross_penalty(crossed)
}

// 0xE0: CPX #imm
pub fn cpx_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.x, v);
}

// 0xE4: CPX zp
pub fn cpx_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.x, v);
}

// 0xEC: CPX abs
pub fn cpx_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.x, v);
}

// 0xC0: CPY #imm
pub fn cpy_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.y, v);
}

// 0xC4: CPY zp
pub fn cpy_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.y, v);
}

// 0xCC: CPY abs
pub fn cpy_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = bus.cpu_read(a);
    cpu.compare(cpu.regs.y, v);
}

// 0xE6: INC zp
pub fn inc_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = cpu.inc_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0xF6: INC zp,X
pub fn inc_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = cpu.inc_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0xEE: INC abs
pub fn inc_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = cpu.inc_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0xFE: INC abs,X
pub fn inc_abs_x(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_x(bus);
    let v = cpu.inc_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0xC6: DEC zp
pub fn dec_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = cpu.dec_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0xD6: DEC zp,X
pub fn dec_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = cpu.dec_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0xCE: DEC abs
pub fn dec_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = cpu.dec_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0xDE: DEC abs,X
pub fn dec_abs_x(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_x(bus);
    let v = cpu.dec_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

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

