use crate::bus::{AccessKind, M68kBus, read_byte, read_word, write_byte};
use crate::ea::{self, Operand, Size};
use crate::instruction::{Context, control_address};
use crate::{Fault, Registers};

fn register(regs: &Registers, index: usize) -> u32 {
    if index < 8 {
        regs.d[index]
    } else {
        regs.address_register(index - 8).unwrap()
    }
}

fn set_register(regs: &mut Registers, index: usize, value: u32) {
    if index < 8 {
        regs.d[index] = value;
    } else {
        regs.set_address_register(index - 8, value);
    }
}

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let op = ctx.opcode;
    if op & 0xf138 == 0x0108 {
        return movep(ctx).map(Some);
    }
    if op & 0xfb80 != 0x4880 {
        return Ok(None);
    }
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if mode == 0 {
        return Ok(None);
    }
    let load = op & 0x400 != 0;
    let valid = if load {
        control_address(mode, reg) || mode == 3
    } else {
        control_address(mode, reg) && !(mode == 7 && reg >= 2) || mode == 4
    };
    if !valid {
        return Err(ctx.unsupported());
    }
    let size = if op & 0x40 == 0 {
        Size::Word
    } else {
        Size::Long
    };
    let mask = ctx.fetch()?;
    let original = *ctx.regs;
    let (mut address, function_code) = if matches!(mode, 3 | 4) {
        (
            ctx.regs.address_register(usize::from(reg)).unwrap(),
            ea::data_space(ctx.regs),
        )
    } else {
        let Operand::Memory {
            address,
            function_code,
            ..
        } = ctx.resolve(mode, reg, size)?
        else {
            return Err(ctx.unsupported());
        };
        (address, function_code)
    };
    for bit in 0..16 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let index = if mode == 4 { 15 - bit } else { bit };
        if mode == 4 {
            address = address.wrapping_sub(size.bytes());
        }
        let operand = Operand::Memory {
            address,
            function_code,
            predecrement: mode == 4,
        };
        if load {
            let value = size.sign_extend(ctx.read(operand, size)?);
            set_register(ctx.regs, index, value);
        } else {
            // On MC68000, a listed predecrement base stores its initial value.
            ctx.write(operand, size, register(&original, index))?;
        }
        if mode != 4 {
            address = address.wrapping_add(size.bytes());
        }
    }
    if load {
        // MC68000 performs a final word read even when no register is selected.
        read_word(ctx.bus, address, function_code, AccessKind::Discard).map_err(Fault::Bus)?;
    }
    if matches!(mode, 3 | 4) {
        ctx.regs.set_address_register(usize::from(reg), address);
    }
    let address_cost = match (mode, reg) {
        (2..=4, _) => 0,
        (5, _) | (7, 0 | 2) => 4,
        (6, _) | (7, 3) => 6,
        (7, 1) => 8,
        _ => return Err(ctx.unsupported()),
    };
    Ok(Some(
        if load { 12 } else { 8 } + address_cost + mask.count_ones() * size.bytes() * 2,
    ))
}

fn movep<B: M68kBus>(ctx: &mut Context<'_, B>) -> Result<u32, Fault<B::Error>> {
    let op = ctx.opcode;
    let index = usize::from((op >> 9) & 7);
    let displacement = i32::from(ctx.fetch()? as i16);
    let base = ctx
        .regs
        .address_register(usize::from(op & 7))
        .unwrap()
        .wrapping_add_signed(displacement);
    let long = op & 0x40 != 0;
    let bytes = if long { 4 } else { 2 };
    let function_code = ea::data_space(ctx.regs);
    let mut value = if op & 0x80 != 0 { ctx.regs.d[index] } else { 0 };
    for position in 0..bytes {
        let address = base.wrapping_add(position * 2);
        if op & 0x80 != 0 {
            let byte = (value >> ((bytes - position - 1) * 8)) as u8;
            write_byte(ctx.bus, address, byte, function_code, AccessKind::Data)
                .map_err(Fault::Bus)?;
        } else {
            let byte =
                read_byte(ctx.bus, address, function_code, AccessKind::Data).map_err(Fault::Bus)?;
            value = value << 8 | u32::from(byte);
        }
    }
    if op & 0x80 == 0 {
        ctx.regs.d[index] = if long {
            value
        } else {
            (ctx.regs.d[index] & 0xffff_0000) | value
        };
    }
    Ok(if long { 24 } else { 16 })
}
