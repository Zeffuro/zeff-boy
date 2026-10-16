use crate::Fault;
use crate::bus::{M68kBus, reset_output, test_and_set_byte};
use crate::constants::{RESET_OUTPUT_CLOCKS, VECTOR_PRIVILEGE_VIOLATION};
use crate::ea::{Operand, Size};
use crate::instruction::{Context, alterable};

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let op = ctx.opcode;
    if op == 0x4e70 {
        if !ctx.regs.supervisor() {
            return ctx.exception(VECTOR_PRIVILEGE_VIOLATION, ctx.pc).map(Some);
        }
        reset_output(ctx.bus, RESET_OUTPUT_CLOCKS).map_err(Fault::Bus)?;
        return Ok(Some(132));
    }
    if op & 0xffc0 != 0x4ac0 {
        return Ok(None);
    }
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if !alterable(mode, reg) {
        return Err(ctx.unsupported());
    }
    let operand = ctx.resolve(mode, reg, Size::Byte)?;
    let value = match operand {
        Operand::DataRegister(_) => {
            let value = ctx.read(operand, Size::Byte)?;
            ctx.write(operand, Size::Byte, value | 0x80)?;
            value
        }
        Operand::Memory {
            address,
            function_code,
            ..
        } => u32::from(test_and_set_byte(ctx.bus, address, function_code).map_err(Fault::Bus)?),
        _ => return Err(ctx.unsupported()),
    };
    crate::alu::logic(ctx.regs, Size::Byte, value);
    Ok(Some(if mode == 0 {
        4
    } else {
        10 + ctx.ea_cycles(mode, reg, Size::Byte)?
    }))
}
