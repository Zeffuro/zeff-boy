use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

#[inline(always)]
fn page_cross_penalty(crossed: bool) -> u8 {
    crossed as u8
}

// 0x29: AND #imm
pub fn and_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x25: AND zp
pub fn and_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x35: AND zp,X
pub fn and_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x2D: AND abs
pub fn and_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x3D: AND abs,X:+1 on page cross
pub fn and_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x39: AND abs,Y:+1 on page cross
pub fn and_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x21: AND (ind,X)
pub fn and_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x31: AND (ind),Y:+1 on page cross
pub fn and_ind_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_indirect_y(bus);
    cpu.regs.a &= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x09: ORA #imm
pub fn ora_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x05: ORA zp
pub fn ora_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x15: ORA zp,X
pub fn ora_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x0D: ORA abs
pub fn ora_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x1D: ORA abs,X:+1 on page cross
pub fn ora_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x19: ORA abs,Y:+1 on page cross
pub fn ora_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x01: ORA (ind,X)
pub fn ora_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x11: ORA (ind),Y:+1 on page cross
pub fn ora_ind_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_indirect_y(bus);
    cpu.regs.a |= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x49: EOR #imm
pub fn eor_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_immediate(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x45: EOR zp
pub fn eor_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x55: EOR zp,X
pub fn eor_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x4D: EOR abs
pub fn eor_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x5D: EOR abs,X:+1 on page cross
pub fn eor_abs_x(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_x(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x59: EOR abs,Y:+1 on page cross
pub fn eor_abs_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_absolute_y(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x41: EOR (ind,X)
pub fn eor_ind_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect_x(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
}

// 0x51: EOR (ind),Y:+1 on page cross
pub fn eor_ind_y(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    let (a, crossed) = cpu.addr_indirect_y(bus);
    cpu.regs.a ^= bus.cpu_read(a);
    cpu.regs.set_zn(cpu.regs.a);
    page_cross_penalty(crossed)
}

// 0x24: BIT zp
pub fn bit_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    cpu.bit_test(bus.cpu_read(a));
}

// 0x2C: BIT abs
pub fn bit_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.bit_test(bus.cpu_read(a));
}

// 0x0A: ASL A
pub fn asl_acc(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.asl_acc();
}

// 0x06: ASL zp
pub fn asl_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = cpu.asl_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x16: ASL zp,X
pub fn asl_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = cpu.asl_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x0E: ASL abs
pub fn asl_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = cpu.asl_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x1E: ASL abs,X
pub fn asl_abs_x(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_x(bus);
    let v = cpu.asl_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x4A: LSR A
pub fn lsr_acc(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.lsr_acc();
}

// 0x46: LSR zp
pub fn lsr_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = cpu.lsr_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x56: LSR zp,X
pub fn lsr_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = cpu.lsr_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x4E: LSR abs
pub fn lsr_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = cpu.lsr_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x5E: LSR abs,X
pub fn lsr_abs_x(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_x(bus);
    let v = cpu.lsr_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x2A: ROL A
pub fn rol_acc(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.rol_acc();
}

// 0x26: ROL zp
pub fn rol_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = cpu.rol_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x36: ROL zp,X
pub fn rol_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = cpu.rol_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x2E: ROL abs
pub fn rol_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = cpu.rol_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x3E: ROL abs,X
pub fn rol_abs_x(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_x(bus);
    let v = cpu.rol_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x6A: ROR A
pub fn ror_acc(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.ror_acc();
}

// 0x66: ROR zp
pub fn ror_zp(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page(bus);
    let v = cpu.ror_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x76: ROR zp,X
pub fn ror_zp_x(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_zero_page_x(bus);
    let v = cpu.ror_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x6E: ROR abs
pub fn ror_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    let v = cpu.ror_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

// 0x7E: ROR abs,X
pub fn ror_abs_x(cpu: &mut Cpu, bus: &mut Bus) {
    let (a, _) = cpu.addr_absolute_x(bus);
    let v = cpu.ror_val(bus.cpu_read(a));
    bus.cpu_write(a, v);
}

