use crate::bus::M68kBus;
use crate::ea::{Operand, Size};
use crate::instruction::{Context, memory_alterable, size};
use crate::{CARRY, EXTEND, Fault, NEGATIVE, OVERFLOW, Registers, ZERO};

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let op = ctx.opcode;
    if op >> 12 != 14 {
        return Ok(None);
    }
    let left = op & 0x100 != 0;
    if let Some(size) = size(op) {
        let index = usize::from(op & 7);
        let count_field = (op >> 9) & 7;
        let count = if op & 0x20 != 0 {
            ctx.regs.d[usize::from(count_field)] & 63
        } else if count_field == 0 {
            8
        } else {
            u32::from(count_field)
        };
        let operand = Operand::DataRegister(index);
        let value = ctx.read(operand, size)?;
        let result = shift(ctx.regs, size, value, count, (op >> 3) & 3, left);
        ctx.write(operand, size, result)?;
        return Ok(Some((if size == Size::Long { 8 } else { 6 }) + 2 * count));
    }
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if op & 0x0800 != 0 || !memory_alterable(mode, reg) {
        return Err(ctx.unsupported());
    }
    let operand = ctx.resolve(mode, reg, Size::Word)?;
    let value = ctx.read(operand, Size::Word)?;
    let result = shift(ctx.regs, Size::Word, value, 1, (op >> 9) & 3, left);
    ctx.write(operand, Size::Word, result)?;
    Ok(Some(8 + ctx.ea_cycles(mode, reg, Size::Word)?))
}

pub(crate) fn shift(
    regs: &mut Registers,
    size: Size,
    value: u32,
    count: u32,
    kind: u16,
    left: bool,
) -> u32 {
    let mut value = size.truncate(value);
    let mut extend = regs.status_register() & EXTEND != 0;
    let mut carry = kind == 2 && extend;
    let mut overflow = false;
    for _ in 0..count {
        let negative = value & size.sign_bit() != 0;
        carry = if left { negative } else { value & 1 != 0 };
        let inserted = match kind {
            0 if !left => negative,
            2 => extend,
            3 => carry,
            _ => false,
        };
        value = if left {
            size.truncate((value << 1) | u32::from(inserted))
        } else {
            (value >> 1) | if inserted { size.sign_bit() } else { 0 }
        };
        if kind == 0 && left {
            overflow |= negative != (value & size.sign_bit() != 0);
        }
        if kind != 3 {
            extend = carry;
        }
    }
    let mut sr = regs.status_register() & !(EXTEND | NEGATIVE | ZERO | OVERFLOW | CARRY);
    if extend {
        sr |= EXTEND;
    }
    if value & size.sign_bit() != 0 {
        sr |= NEGATIVE;
    }
    if value == 0 {
        sr |= ZERO;
    }
    if overflow {
        sr |= OVERFLOW;
    }
    if carry {
        sr |= CARRY;
    }
    regs.set_status_register(sr);
    value
}
