use crate::alu::condition;
use crate::bus::{AccessKind, M68kBus, read_word};
use crate::constants::{
    CCR_MASK, VECTOR_ILLEGAL_INSTRUCTION, VECTOR_LINE_A, VECTOR_LINE_F, VECTOR_PRIVILEGE_VIOLATION,
    VECTOR_TRAP_BASE, VECTOR_TRAPV,
};
use crate::ea::{Operand, Size};
use crate::instruction::{Context, alterable, control_address, data_source};
use crate::{Fault, OVERFLOW, TRACE};

pub(crate) fn execute<B: M68kBus>(
    ctx: &mut Context<'_, B>,
) -> Result<Option<u32>, Fault<B::Error>> {
    let op = ctx.opcode;
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if op & 0xf000 == 0x6000 {
        let base = ctx.cursor;
        let word = op as u8 == 0;
        let displacement = if word {
            i32::from(ctx.fetch()? as i16)
        } else {
            i32::from(op as u8 as i8)
        };
        let code = (op >> 8) & 15;
        if code == 1 {
            ctx.push(ctx.cursor)?;
            ctx.next_pc = Some(base.wrapping_add_signed(displacement));
            return Ok(Some(18));
        }
        let taken = condition(ctx.regs.status_register(), code);
        if taken {
            ctx.next_pc = Some(base.wrapping_add_signed(displacement));
        }
        return Ok(Some(if taken {
            10
        } else if word {
            12
        } else {
            8
        }));
    }
    if op & 0xf0f8 == 0x50c8 {
        let base = ctx.cursor;
        let displacement = i32::from(ctx.fetch()? as i16);
        if condition(ctx.regs.status_register(), (op >> 8) & 15) {
            return Ok(Some(12));
        }
        let index = usize::from(reg);
        let counter = (ctx.regs.d[index] as u16).wrapping_sub(1);
        ctx.regs.d[index] = (ctx.regs.d[index] & 0xffff_0000) | u32::from(counter);
        if counter != u16::MAX {
            ctx.next_pc = Some(base.wrapping_add_signed(displacement));
            return Ok(Some(10));
        }
        return Ok(Some(14));
    }
    if op & 0xf0c0 == 0x50c0 {
        if !alterable(mode, reg) {
            return Err(ctx.unsupported());
        }
        let value = if condition(ctx.regs.status_register(), (op >> 8) & 15) {
            0xff
        } else {
            0
        };
        let operand = ctx.resolve(mode, reg, Size::Byte)?;
        if mode != 0 {
            ctx.read(operand, Size::Byte)?;
        }
        ctx.write(operand, Size::Byte, value)?;
        return Ok(Some(if mode == 0 {
            if value == 0 { 4 } else { 6 }
        } else {
            8 + ctx.ea_cycles(mode, reg, Size::Byte)?
        }));
    }
    if op & 0xf1c0 == 0x41c0 || op & 0xffc0 == 0x4840 || matches!(op & 0xffc0, 0x4e80 | 0x4ec0) {
        if !control_address(mode, reg) {
            return Ok(None);
        }
        let operand = ctx.resolve(mode, reg, Size::Long)?;
        let Operand::Memory { address, .. } = operand else {
            return Err(ctx.unsupported());
        };
        let address_cost = match (mode, reg) {
            (2, _) => 0,
            (5, _) | (7, 0 | 2) => 4,
            (6, _) | (7, 3) => 8,
            (7, 1) => 8,
            _ => return Err(ctx.unsupported()),
        };
        if op & 0xf1c0 == 0x41c0 {
            ctx.regs
                .set_address_register(usize::from((op >> 9) & 7), address);
            return Ok(Some(4 + address_cost));
        }
        if op & 0xffc0 == 0x4840 {
            ctx.push(address)?;
            return Ok(Some(12 + address_cost));
        }
        let jump_cost = match (mode, reg) {
            (2, _) => 8,
            (5, _) | (7, 0 | 2) => 10,
            (6, _) | (7, 3) => 14,
            (7, 1) => 12,
            _ => return Err(ctx.unsupported()),
        };
        if op & 0xffc0 == 0x4e80 {
            ctx.push(ctx.cursor)?;
        }
        ctx.next_pc = Some(address);
        return Ok(Some(jump_cost + if op & 0xffc0 == 0x4e80 { 8 } else { 0 }));
    }
    if op & 0xfff8 == 0x4e50 {
        let displacement = i32::from(ctx.fetch()? as i16);
        let old = ctx.regs.address_register(usize::from(reg)).unwrap();
        ctx.push(old)?;
        let frame = ctx.regs.stack_pointer();
        ctx.regs.set_address_register(usize::from(reg), frame);
        ctx.regs
            .set_address_register(7, frame.wrapping_add_signed(displacement));
        return Ok(Some(16));
    }
    if op & 0xfff8 == 0x4e58 {
        let frame = ctx.regs.address_register(usize::from(reg)).unwrap();
        ctx.regs.set_address_register(7, frame);
        let value = ctx.pop()?;
        ctx.regs.set_address_register(usize::from(reg), value);
        return Ok(Some(12));
    }
    if op == 0x4e75 {
        ctx.next_pc = Some(ctx.pop()?);
        return Ok(Some(16));
    }
    if op == 0x4e77 {
        let address = ctx.regs.stack_pointer();
        let ccr = read_word(
            ctx.bus,
            address,
            crate::ea::data_space(ctx.regs),
            AccessKind::Stack,
        )
        .map_err(Fault::Bus)?;
        ctx.regs.set_address_register(7, address.wrapping_add(2));
        ctx.next_pc = Some(ctx.pop()?);
        ctx.regs
            .set_status_register((ctx.regs.status_register() & !CCR_MASK) | (ccr & CCR_MASK));
        return Ok(Some(20));
    }
    if matches!(op & 0xf1f8, 0xc140 | 0xc148 | 0xc188) {
        let left = usize::from((op >> 9) & 7);
        let right = usize::from(reg);
        match op & 0xf1f8 {
            0xc140 => ctx.regs.d.swap(left, right),
            0xc148 => {
                let a = ctx.regs.address_register(left).unwrap();
                let b = ctx.regs.address_register(right).unwrap();
                ctx.regs.set_address_register(left, b);
                ctx.regs.set_address_register(right, a);
            }
            _ => {
                let a = ctx.regs.d[left];
                let b = ctx.regs.address_register(right).unwrap();
                ctx.regs.d[left] = b;
                ctx.regs.set_address_register(right, a);
            }
        }
        return Ok(Some(6));
    }
    if op & 0xfff0 == 0x4e40 {
        return ctx
            .exception(VECTOR_TRAP_BASE + (op as u8 & 15), ctx.cursor)
            .map(Some);
    }
    if matches!(op, 0x4afa..=0x4afc) {
        return ctx.exception(VECTOR_ILLEGAL_INSTRUCTION, ctx.pc).map(Some);
    }
    if matches!(op >> 12, 10 | 15) {
        return ctx
            .exception(
                if op >> 12 == 10 {
                    VECTOR_LINE_A
                } else {
                    VECTOR_LINE_F
                },
                ctx.pc,
            )
            .map(Some);
    }
    if op == 0x4e76 {
        return if ctx.regs.status_register() & OVERFLOW != 0 {
            ctx.exception(VECTOR_TRAPV, ctx.cursor).map(Some)
        } else {
            Ok(Some(4))
        };
    }
    if matches!(op, 0x4e72 | 0x4e73) || op & 0xfff0 == 0x4e60 {
        if !ctx.regs.supervisor() {
            return ctx.exception(VECTOR_PRIVILEGE_VIOLATION, ctx.pc).map(Some);
        }
        match op {
            0x4e72 => {
                let sr = ctx.fetch()?;
                if sr & TRACE != 0 {
                    return Err(Fault::TraceUnsupported);
                }
                ctx.regs.set_status_register(sr);
                ctx.stopped = true;
                return Ok(Some(4));
            }
            0x4e73 => {
                crate::exception::return_from_exception(ctx.regs, ctx.bus)?;
                ctx.next_pc = Some(ctx.regs.pc);
                return Ok(Some(20));
            }
            _ if op & 8 == 0 => ctx.regs.usp = ctx.regs.address_register(usize::from(reg)).unwrap(),
            _ => {
                ctx.regs
                    .set_address_register(usize::from(reg), ctx.regs.usp);
            }
        }
        return Ok(Some(4));
    }
    if matches!(op, 0x003c | 0x023c | 0x0a3c | 0x007c | 0x027c | 0x0a7c) {
        let full = op & 0x40 != 0;
        if full && !ctx.regs.supervisor() {
            return ctx.exception(VECTOR_PRIVILEGE_VIOLATION, ctx.pc).map(Some);
        }
        let immediate = ctx.fetch()?;
        let mask = if full { u16::MAX } else { CCR_MASK };
        let sr = ctx.regs.status_register();
        let value = match op & 0x0f00 {
            0 => sr | (immediate & mask),
            0x0200 => sr & (immediate | !mask),
            _ => sr ^ (immediate & mask),
        };
        ctx.regs.set_status_register(value);
        return Ok(Some(20));
    }
    if matches!(op & 0xffc0, 0x40c0 | 0x44c0 | 0x46c0) {
        let from_sr = op & 0xffc0 == 0x40c0;
        let to_sr = op & 0xffc0 == 0x46c0;
        if if from_sr {
            !alterable(mode, reg)
        } else {
            !data_source(mode, reg)
        } {
            return Err(ctx.unsupported());
        }
        if to_sr && !ctx.regs.supervisor() {
            return ctx.exception(VECTOR_PRIVILEGE_VIOLATION, ctx.pc).map(Some);
        }
        let operand = ctx.resolve(mode, reg, Size::Word)?;
        if from_sr {
            if mode != 0 {
                ctx.read(operand, Size::Word)?;
            }
            ctx.write(operand, Size::Word, u32::from(ctx.regs.status_register()))?;
            return Ok(Some(if mode == 0 {
                6
            } else {
                8 + ctx.ea_cycles(mode, reg, Size::Word)?
            }));
        }
        let value = ctx.read(operand, Size::Word)? as u16;
        ctx.regs.set_status_register(if to_sr {
            value
        } else {
            (ctx.regs.status_register() & !CCR_MASK) | (value & CCR_MASK)
        });
        return Ok(Some(12 + ctx.ea_cycles(mode, reg, Size::Word)?));
    }
    Ok(None)
}
