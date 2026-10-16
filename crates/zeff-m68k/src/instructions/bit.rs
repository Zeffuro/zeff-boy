use crate::bus::M68kBus;
use crate::ea::Size;
use crate::instruction::{Context, alterable, data_source};
use crate::{Fault, ZERO};

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let op = ctx.opcode;
    let immediate = op & 0xff00 == 0x0800;
    if !immediate && op & 0xf100 != 0x0100 {
        return Ok(None);
    }
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    // Dynamic mode 1 belongs to MOVEP rather than the bit instructions.
    if !immediate && mode == 1 {
        return Ok(None);
    }
    let operation = (op >> 6) & 3;
    let valid = if operation == 0 {
        data_source(mode, reg) && !(immediate && mode == 7 && reg == 4)
    } else {
        alterable(mode, reg)
    };
    if !valid {
        return Err(ctx.unsupported());
    }
    let bit = if immediate {
        u32::from(ctx.fetch()?)
    } else {
        ctx.regs.d[usize::from((op >> 9) & 7)]
    };
    let size = if mode == 0 { Size::Long } else { Size::Byte };
    let mask = 1 << (bit & if mode == 0 { 31 } else { 7 });
    let operand = ctx.resolve(mode, reg, size)?;
    let value = ctx.read(operand, size)?;
    let sr = (ctx.regs.status_register() & !ZERO) | if value & mask == 0 { ZERO } else { 0 };
    ctx.regs.set_status_register(sr);
    let result = match operation {
        1 => value ^ mask,
        2 => value & !mask,
        3 => value | mask,
        _ => value,
    };
    if operation != 0 {
        ctx.write(operand, size, result)?;
    }
    // Register modifications take two extra clocks for the high 16-bit half.
    let base = if mode == 0 || mode == 7 && reg == 4 {
        match operation {
            0 => 6,
            2 => 8 + if bit & 16 != 0 { 2 } else { 0 },
            _ => 6 + if bit & 16 != 0 { 2 } else { 0 },
        }
    } else if operation == 0 {
        4
    } else {
        8
    };
    Ok(Some(
        base + if immediate { 4 } else { 0 } + ctx.ea_cycles(mode, reg, size)?,
    ))
}
