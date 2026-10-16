use crate::Fault;
use crate::alu;
use crate::bus::M68kBus;
use crate::ea::{Operand, Size};
use crate::instruction::{Context, alterable, data_source, memory_alterable, size};

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let op = ctx.opcode;
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let family = op >> 12;
    if let Some(cycles) = crate::extend::execute(ctx)? {
        return Ok(Some(cycles));
    }
    if let Some(cycles) = crate::bcd::execute(ctx)? {
        return Ok(Some(cycles));
    }
    if (1..=3).contains(&family) {
        return move_operand(ctx).map(Some);
    }
    if family == 5
        && let Some(size) = size(op)
    {
        if !alterable(mode, reg) && !(mode == 1 && size != Size::Byte) {
            return Err(ctx.unsupported());
        }
        let quick = match (op >> 9) & 7 {
            0 => 8,
            value => u32::from(value),
        };
        let operand = ctx.resolve(mode, reg, size)?;
        if mode == 1 {
            let value = ctx.regs.address_register(usize::from(reg)).unwrap();
            let value = if op & 0x100 == 0 {
                value.wrapping_add(quick)
            } else {
                value.wrapping_sub(quick)
            };
            ctx.regs.set_address_register(usize::from(reg), value);
            return Ok(Some(8));
        }
        let value = ctx.read(operand, size)?;
        let result = if op & 0x100 == 0 {
            alu::add(ctx.regs, size, value, quick)
        } else {
            alu::subtract(ctx.regs, size, value, quick, true)
        };
        ctx.write(operand, size, result)?;
        let cycles = if mode == 0 {
            if size == Size::Long { 8 } else { 4 }
        } else {
            (if size == Size::Long { 12 } else { 8 }) + ctx.ea_cycles(mode, reg, size)?
        };
        return Ok(Some(cycles));
    }
    if family == 0 && matches!((op >> 8) & 15, 0 | 2 | 4 | 6 | 10 | 12) {
        return immediate(ctx).map(Some);
    }
    if matches!(family, 8 | 9 | 11 | 12 | 13) {
        return binary(ctx).map(Some);
    }
    if op & 0xfff8 == 0x4840 {
        let index = usize::from(reg);
        let value = ctx.regs.d[index].rotate_left(16);
        ctx.regs.d[index] = value;
        alu::logic(ctx.regs, Size::Long, value);
        return Ok(Some(4));
    }
    if matches!(op & 0xfff8, 0x4880 | 0x48c0) {
        let operand = Operand::DataRegister(usize::from(reg));
        let (source, destination) = if op & 0x40 == 0 {
            (Size::Byte, Size::Word)
        } else {
            (Size::Word, Size::Long)
        };
        let value = source.sign_extend(ctx.read(operand, source)?);
        ctx.write(operand, destination, value)?;
        alu::logic(ctx.regs, destination, value);
        return Ok(Some(4));
    }
    if matches!(op & 0xff00, 0x4200 | 0x4400 | 0x4600 | 0x4a00) {
        let Some(size) = size(op) else {
            return Ok(None);
        };
        if !alterable(mode, reg) {
            return Err(ctx.unsupported());
        }
        let operand = ctx.resolve(mode, reg, size)?;
        // Even CLR performs the original 68000 read before its write.
        let value = ctx.read(operand, size)?;
        let result = match op & 0xff00 {
            0x4200 => 0,
            0x4400 => alu::subtract(ctx.regs, size, 0, value, true),
            0x4600 => size.truncate(!value),
            _ => value,
        };
        if op & 0xff00 != 0x4400 {
            alu::logic(ctx.regs, size, result);
        }
        let test = op & 0xff00 == 0x4a00;
        if !test {
            ctx.write(operand, size, result)?;
        }
        let cycles = if test {
            4 + ctx.ea_cycles(mode, reg, size)?
        } else if mode == 0 {
            if size == Size::Long { 6 } else { 4 }
        } else {
            (if size == Size::Long { 12 } else { 8 }) + ctx.ea_cycles(mode, reg, size)?
        };
        return Ok(Some(cycles));
    }
    Ok(None)
}

fn move_operand<B: M68kBus>(ctx: &mut Context<'_, B>) -> Result<u32, Fault<B::Error>> {
    let op = ctx.opcode;
    let size = match op >> 12 {
        1 => Size::Byte,
        2 => Size::Long,
        _ => Size::Word,
    };
    let source_mode = (op >> 3) & 7;
    let source_reg = op & 7;
    let destination_mode = (op >> 6) & 7;
    let destination_reg = (op >> 9) & 7;
    if (!data_source(source_mode, source_reg) && source_mode != 1)
        || source_mode == 1 && size == Size::Byte
        || (!alterable(destination_mode, destination_reg) && destination_mode != 1)
        || destination_mode == 1 && size == Size::Byte
    {
        return Err(ctx.unsupported());
    }
    let source = ctx.resolve(source_mode, source_reg, size)?;
    let value = ctx.read(source, size)?;
    let destination = ctx.resolve(destination_mode, destination_reg, size)?;
    ctx.write(destination, size, value)?;
    if destination_mode != 1 {
        alu::logic(ctx.regs, size, value);
    }
    let source_cycles = ctx.ea_cycles(source_mode, source_reg, size)?;
    let destination_cycles = ctx.ea_cycles(destination_mode, destination_reg, size)?
        - if destination_mode == 4 { 2 } else { 0 };
    Ok(4 + source_cycles + destination_cycles)
}

fn immediate<B: M68kBus>(ctx: &mut Context<'_, B>) -> Result<u32, Fault<B::Error>> {
    let op = ctx.opcode;
    let size = size(op).ok_or_else(|| ctx.unsupported())?;
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if !alterable(mode, reg) {
        return Err(ctx.unsupported());
    }
    let immediate = ctx.resolve(7, 4, size)?;
    let source = ctx.read(immediate, size)?;
    let destination = ctx.resolve(mode, reg, size)?;
    let old = ctx.read(destination, size)?;
    let kind = (op >> 8) & 15;
    let result = match kind {
        0 => old | source,
        2 => old & source,
        4 | 12 => alu::subtract(ctx.regs, size, old, source, kind != 12),
        6 => alu::add(ctx.regs, size, old, source),
        10 => old ^ source,
        _ => unreachable!(),
    };
    if matches!(kind, 0 | 2 | 10) {
        alu::logic(ctx.regs, size, result);
    }
    if kind != 12 {
        ctx.write(destination, size, result)?;
    }
    let cycles = if mode == 0 {
        if size != Size::Long {
            8
        } else if kind == 12 {
            14
        } else {
            16
        }
    } else {
        let base = if kind == 12 {
            if size == Size::Long { 12 } else { 8 }
        } else if size == Size::Long {
            20
        } else {
            12
        };
        base + ctx.ea_cycles(mode, reg, size)?
    };
    Ok(cycles)
}

fn binary<B: M68kBus>(ctx: &mut Context<'_, B>) -> Result<u32, Fault<B::Error>> {
    let op = ctx.opcode;
    let family = op >> 12;
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let destination_reg = usize::from((op >> 9) & 7);
    let opmode = (op >> 6) & 7;
    if matches!(opmode, 3 | 7) && matches!(family, 9 | 11 | 13) {
        let size = if opmode == 3 { Size::Word } else { Size::Long };
        if !data_source(mode, reg) && mode != 1 {
            return Err(ctx.unsupported());
        }
        let source = ctx.resolve(mode, reg, size)?;
        let value = size.sign_extend(ctx.read(source, size)?);
        let old = ctx.regs.address_register(destination_reg).unwrap();
        if family == 11 {
            alu::subtract(ctx.regs, Size::Long, old, value, false);
        } else {
            let result = if family == 9 {
                old.wrapping_sub(value)
            } else {
                old.wrapping_add(value)
            };
            ctx.regs.set_address_register(destination_reg, result);
        }
        let base = if family == 11 {
            6
        } else if size == Size::Word || matches!(mode, 0 | 1) || mode == 7 && reg == 4 {
            8
        } else {
            6
        };
        return Ok(base + ctx.ea_cycles(mode, reg, size)?);
    }
    let size = size(op).ok_or_else(|| ctx.unsupported())?;
    if op & 0xf138 == 0xb108 {
        let source = ctx.resolve(3, reg, size)?;
        let value = ctx.read(source, size)?;
        let destination = ctx.resolve(3, destination_reg as u16, size)?;
        let old = ctx.read(destination, size)?;
        alu::subtract(ctx.regs, size, old, value, false);
        return Ok(if size == Size::Long { 20 } else { 12 });
    }
    let to_memory = opmode >= 4;
    if to_memory {
        // Mode 0/1 aliases extended arithmetic, BCD and EXG.
        if !memory_alterable(mode, reg) && !(family == 11 && mode == 0) {
            return Err(ctx.unsupported());
        }
    } else if !data_source(mode, reg)
        && !(matches!(family, 9 | 11 | 13) && mode == 1 && size != Size::Byte)
    {
        return Err(ctx.unsupported());
    }
    let ea = ctx.resolve(mode, reg, size)?;
    let data = Operand::DataRegister(destination_reg);
    let (source, destination) = if to_memory { (data, ea) } else { (ea, data) };
    let source = ctx.read(source, size)?;
    let old = ctx.read(destination, size)?;
    let result = match family {
        8 => old | source,
        9 => alu::subtract(ctx.regs, size, old, source, true),
        11 if to_memory => old ^ source,
        11 => alu::subtract(ctx.regs, size, old, source, false),
        12 => old & source,
        13 => alu::add(ctx.regs, size, old, source),
        _ => unreachable!(),
    };
    if matches!(family, 8 | 12) || family == 11 && to_memory {
        alu::logic(ctx.regs, size, result);
    }
    if family != 11 || to_memory {
        ctx.write(destination, size, result)?;
    }
    let cycles = if to_memory {
        if mode == 0 {
            if size == Size::Long { 8 } else { 4 }
        } else {
            (if size == Size::Long { 12 } else { 8 }) + ctx.ea_cycles(mode, reg, size)?
        }
    } else {
        let base = if size != Size::Long {
            4
        } else if family == 11 {
            6
        } else if matches!(mode, 0 | 1) || mode == 7 && reg == 4 {
            8
        } else {
            6
        };
        base + ctx.ea_cycles(mode, reg, size)?
    };
    Ok(cycles)
}
