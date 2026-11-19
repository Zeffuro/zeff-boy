use crate::bus::M68kBus;
use crate::constants::CCR_MASK;
use crate::ea::{Operand, Size};
use crate::instruction::{Context, alterable, size};
use crate::{CARRY, EXTEND, Fault, NEGATIVE, OVERFLOW, Registers, ZERO};

pub(crate) fn arithmetic(
    regs: &mut Registers,
    size: Size,
    left: u32,
    right: u32,
    subtract: bool,
) -> u32 {
    let old = regs.status_register();
    let x = u64::from(old & EXTEND != 0);
    let a = u64::from(size.truncate(left));
    let b = u64::from(size.truncate(right));
    let signed = |value: u32| i64::from(size.sign_extend(value) as i32);
    let (wide, result, carry) = if subtract {
        (
            signed(left) - signed(right) - x as i64,
            size.truncate(a.wrapping_sub(b + x) as u32),
            a < b + x,
        )
    } else {
        (
            signed(left) + signed(right) + x as i64,
            size.truncate((a + b + x) as u32),
            a + b + x > u64::from(size.mask()),
        )
    };
    let bound = i64::from(size.sign_bit());
    let flags = if carry { CARRY | EXTEND } else { 0 }
        | if wide < -bound || wide >= bound {
            OVERFLOW
        } else {
            0
        }
        | if result & size.sign_bit() != 0 {
            NEGATIVE
        } else {
            0
        }
        | if result == 0 && old & ZERO != 0 {
            ZERO
        } else {
            0
        };
    regs.set_status_register((old & !CCR_MASK) | flags);
    result
}

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let op = ctx.opcode;
    if matches!(op & 0xf130, 0xd100 | 0x9100) {
        let Some(size) = size(op) else {
            return Ok(None);
        };
        let memory = op & 8 != 0;
        let source = if memory {
            ctx.resolve(4, op & 7, size)?
        } else {
            Operand::DataRegister(usize::from(op & 7))
        };
        let value = ctx.read(source, size)?;
        let destination = if memory {
            ctx.resolve(4, (op >> 9) & 7, size)?
        } else {
            Operand::DataRegister(usize::from((op >> 9) & 7))
        };
        let old = ctx.read(destination, size)?;
        let result = arithmetic(ctx.regs, size, old, value, op & 0xf000 == 0x9000);
        ctx.write(destination, size, result)?;
        return Ok(Some(if memory {
            if size == Size::Long { 30 } else { 18 }
        } else if size == Size::Long {
            8
        } else {
            4
        }));
    }
    if op & 0xff00 == 0x4000 {
        let Some(size) = size(op) else {
            return Ok(None);
        };
        let mode = (op >> 3) & 7;
        let reg = op & 7;
        if !alterable(mode, reg) {
            return Err(ctx.unsupported());
        }
        let operand = ctx.resolve(mode, reg, size)?;
        let value = ctx.read(operand, size)?;
        let result = arithmetic(ctx.regs, size, 0, value, true);
        ctx.write(operand, size, result)?;
        return Ok(Some(if mode == 0 {
            if size == Size::Long { 6 } else { 4 }
        } else {
            (if size == Size::Long { 12 } else { 8 }) + ctx.ea_cycles(mode, reg, size)?
        }));
    }
    Ok(None)
}
