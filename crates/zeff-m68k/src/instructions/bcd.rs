use crate::bus::M68kBus;
use crate::constants::CCR_MASK;
use crate::ea::{Operand, Size};
use crate::instruction::{Context, alterable};
use crate::{CARRY, EXTEND, Fault, NEGATIVE, OVERFLOW, Registers, ZERO};

pub(crate) fn arithmetic(regs: &mut Registers, destination: u8, source: u8, subtract: bool) -> u8 {
    let old = regs.status_register();
    let extend = i16::from(old & EXTEND != 0);
    let left = i16::from(destination);
    let right = i16::from(source);
    let raw = if subtract {
        left - right - extend
    } else {
        left + right + extend
    };
    let byte = raw as u8;
    let (result, carry, overflow) = if subtract {
        let low_borrow = (left & 15) - (right & 15) - extend < 0;
        let correction = u16::from(low_borrow) * 6 + u16::from(raw < 0) * 96;
        let result = byte.wrapping_sub(correction as u8);
        (
            result,
            raw < 0 || u16::from(byte) < correction,
            byte >= 128 && result < 128,
        )
    } else {
        let low_carry = (left & 15) + (right & 15) + extend > 9;
        let carry = raw > 255 || byte > 0x99;
        let correction = u8::from(low_carry) * 6 + u8::from(carry) * 96;
        let result = byte.wrapping_add(correction);
        (result, carry, byte < 128 && result >= 128)
    };
    // Invalid BCD digits retain the 68000 N/V behavior.
    let flags = if carry { CARRY | EXTEND } else { 0 }
        | if overflow { OVERFLOW } else { 0 }
        | if result >= 128 { NEGATIVE } else { 0 }
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
    if matches!(op & 0xf1f0, 0xc100 | 0x8100) {
        let source_reg = op & 7;
        let destination_reg = (op >> 9) & 7;
        let memory = op & 8 != 0;
        let source = if memory {
            ctx.resolve(4, source_reg, Size::Byte)?
        } else {
            Operand::DataRegister(usize::from(source_reg))
        };
        let value = ctx.read(source, Size::Byte)? as u8;
        let destination = if memory {
            ctx.resolve(4, destination_reg, Size::Byte)?
        } else {
            Operand::DataRegister(usize::from(destination_reg))
        };
        let old = ctx.read(destination, Size::Byte)? as u8;
        let result = arithmetic(ctx.regs, old, value, op & 0xf000 == 0x8000);
        ctx.write(destination, Size::Byte, u32::from(result))?;
        return Ok(Some(if memory { 18 } else { 6 }));
    }
    if op & 0xffc0 == 0x4800 {
        let mode = (op >> 3) & 7;
        let reg = op & 7;
        if !alterable(mode, reg) {
            return Err(ctx.unsupported());
        }
        let operand = ctx.resolve(mode, reg, Size::Byte)?;
        let value = ctx.read(operand, Size::Byte)? as u8;
        let result = arithmetic(ctx.regs, 0, value, true);
        ctx.write(operand, Size::Byte, u32::from(result))?;
        return Ok(Some(if mode == 0 {
            6
        } else {
            8 + ctx.ea_cycles(mode, reg, Size::Byte)?
        }));
    }
    Ok(None)
}
