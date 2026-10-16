use super::{Case, Phase, memory_map};
use std::collections::BTreeMap;
use zeff_m68k::{Registers, SUPERVISOR, TRACE};

pub(super) struct ExceptionContract {
    pub(super) vector: u8,
    pub(super) stack: [u32; 3],
}

fn word(memory: &BTreeMap<u32, u8>, address: u32) -> Result<u16, String> {
    let address = address & 0x00ff_ffff;
    let byte = |address| {
        memory
            .get(&address)
            .copied()
            .ok_or_else(|| format!("unobserved arithmetic witness read {address:06x}"))
    };
    Ok(u16::from(byte(address)?) << 8 | u16::from(byte(address.wrapping_add(1) & 0x00ff_ffff)?))
}

fn put_word(memory: &mut BTreeMap<u32, u8>, address: u32, value: u16) {
    memory.insert(address & 0x00ff_ffff, (value >> 8) as u8);
    memory.insert(address.wrapping_add(1) & 0x00ff_ffff, value as u8);
}

fn source_word(
    case: &Case,
    regs: &Registers,
    memory: &BTreeMap<u32, u8>,
) -> Result<(u16, u32, u32), String> {
    let op = case.initial.prefetch[0];
    let mode = (op >> 3) & 7;
    let register = usize::from(op & 7);
    let extension = case.initial.prefetch[1];
    let base_pc = regs.pc.wrapping_add(2);
    let an = |index: usize| {
        if index == 7 {
            regs.stack_pointer()
        } else {
            regs.a[index]
        }
    };
    let index = || {
        let number = usize::from((extension >> 12) & 7);
        let value = if extension & 0x8000 != 0 {
            an(number)
        } else {
            regs.d[number]
        };
        if extension & 0x0800 != 0 {
            value as i32
        } else {
            i32::from(value as i16)
        }
    };
    let mut stack = regs.ssp;
    let (address, extensions) = match mode {
        0 => return Ok((regs.d[register] as u16, base_pc, stack)),
        2 | 3 => {
            if mode == 3 && register == 7 && regs.supervisor() {
                stack = stack.wrapping_add(2);
            }
            (an(register), 0)
        }
        4 => {
            if register == 7 && regs.supervisor() {
                stack = stack.wrapping_sub(2);
            }
            (an(register).wrapping_sub(2), 0)
        }
        5 => (
            an(register).wrapping_add_signed(i32::from(extension as i16)),
            1,
        ),
        6 => (
            an(register)
                .wrapping_add_signed(index())
                .wrapping_add_signed(i32::from(extension as u8 as i8)),
            1,
        ),
        7 => match register {
            0 => (i32::from(extension as i16) as u32, 1),
            1 => (
                (u32::from(extension) << 16) | u32::from(word(memory, regs.pc.wrapping_add(4))?),
                2,
            ),
            2 => (base_pc.wrapping_add_signed(i32::from(extension as i16)), 1),
            3 => (
                base_pc
                    .wrapping_add_signed(index())
                    .wrapping_add_signed(i32::from(extension as u8 as i8)),
                1,
            ),
            4 => return Ok((extension, base_pc.wrapping_add(2), stack)),
            _ => return Err("invalid arithmetic source form".into()),
        },
        _ => return Err("invalid arithmetic source form".into()),
    };
    Ok((
        word(memory, address)?,
        base_pc.wrapping_add(extensions * 2),
        stack,
    ))
}

pub(super) fn exception_contract(case: &Case) -> Result<Option<ExceptionContract>, String> {
    let family = match case.cohort.as_str() {
        "MULU" => 0xc0c0,
        "MULS" => 0xc1c0,
        "DIVU" => 0x80c0,
        "DIVS" => 0x81c0,
        "CHK" => 0x4180,
        _ => return Ok(None),
    };
    let opcode = case.initial.prefetch[0];
    if opcode & 0xf1c0 != family {
        return Err("arithmetic opcode contradicts cohort".into());
    }
    let regs = case.initial.registers()?;
    let mut memory = memory_map(&case.initial.ram)?;
    let (source, saved_pc, stack_pointer) = source_word(case, &regs, &memory)?;
    let value = regs.d[usize::from((opcode >> 9) & 7)];
    let (vector, flag_value) = if family == 0x4180 {
        let operand = i32::from(source as i16);
        let destination = i32::from(value as i16);
        let delta = operand - destination;
        if (0..=32767).contains(&delta) && destination >= 0 {
            return Ok(None);
        }
        (6, value as u16)
    } else if matches!(family, 0x80c0 | 0x81c0) && source == 0 {
        (
            5,
            if family == 0x80c0 {
                (value >> 16) as u16
            } else {
                0
            },
        )
    } else {
        return Ok(None);
    };
    let saved_sr = (regs.status_register() & !0x000f)
        | if flag_value & 0x8000 != 0 { 8 } else { 0 }
        | if flag_value == 0 { 4 } else { 0 };
    let stack = [2, 6, 4].map(|offset| stack_pointer.wrapping_sub(offset) & 0x00ff_ffff);
    let values = [saved_pc as u16, saved_sr, (saved_pc >> 16) as u16];
    let writes: Vec<_> = case
        .transactions
        .iter()
        .filter_map(|phase| {
            if let Phase::Write(transfer) = phase {
                Some(transfer)
            } else {
                None
            }
        })
        .collect();
    if writes.len() != 3
        || writes
            .iter()
            .zip(stack.into_iter().zip(values))
            .any(|(transfer, (address, data))| {
                transfer.address_bus != address
                    || transfer.function_code != 5
                    || transfer.uds != 1
                    || transfer.lds != 1
                    || transfer.data_bus != u32::from(data)
            })
    {
        return Err("arithmetic exception frame contradicts opcode and initial state".into());
    }
    for (address, data) in stack.into_iter().zip(values) {
        put_word(&mut memory, address, data);
    }
    let vector_address = u32::from(vector) * 4;
    let handler = (u32::from(word(&memory, vector_address)?) << 16)
        | u32::from(word(&memory, vector_address + 2)?);
    let expected = case.expected.registers()?;
    if expected.ssp != stack_pointer.wrapping_sub(6)
        || expected.status_register() != (saved_sr | SUPERVISOR) & !TRACE
        || expected.pc != handler
        || case.expected.prefetch
            != [
                word(&memory, handler)?,
                word(&memory, handler.wrapping_add(2))?,
            ]
    {
        return Err("arithmetic exception boundary contradicts initial state".into());
    }
    Ok(Some(ExceptionContract { vector, stack }))
}
