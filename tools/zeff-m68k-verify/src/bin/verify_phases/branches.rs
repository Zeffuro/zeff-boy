use super::{Case, Phase, memory_map};
use zeff_m68k::Registers;

pub(super) struct StackContract {
    pub(super) addresses: [u32; 2],
    pub(super) function_code: u32,
}

fn condition(code: u16, sr: u16) -> bool {
    let (c, z, n, v) = (sr & 1 != 0, sr & 4 != 0, sr & 8 != 0, sr & 2 != 0);
    [
        true,
        false,
        !(c || z),
        c || z,
        !c,
        c,
        !z,
        z,
        !v,
        v,
        !n,
        n,
        n == v,
        n != v,
        !z && n == v,
        z || n != v,
    ][usize::from(code)]
}

fn address(case: &Case, regs: &Registers) -> Result<(u32, u32), String> {
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
    let (target, words) = match mode {
        2 => (an(register), 0),
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
            1 => {
                let memory = memory_map(&case.initial.ram)?;
                let cursor = regs.pc.wrapping_add(4) & 0x00ff_ffff;
                let high = *memory
                    .get(&cursor)
                    .ok_or("unobserved control extension high byte")?;
                let low = *memory
                    .get(&(cursor.wrapping_add(1) & 0x00ff_ffff))
                    .ok_or("unobserved control extension low byte")?;
                (
                    (u32::from(extension) << 16) | (u32::from(high) << 8) | u32::from(low),
                    2,
                )
            }
            2 => (base_pc.wrapping_add_signed(i32::from(extension as i16)), 1),
            3 => (
                base_pc
                    .wrapping_add_signed(index())
                    .wrapping_add_signed(i32::from(extension as u8 as i8)),
                1,
            ),
            _ => return Err("invalid control address form".into()),
        },
        _ => return Err("invalid control address form".into()),
    };
    Ok((target, base_pc.wrapping_add(words * 2)))
}

pub(super) fn contract(case: &Case) -> Result<Option<StackContract>, String> {
    if !matches!(
        case.cohort.as_str(),
        "Bcc" | "BSR" | "DBcc" | "Scc" | "JMP" | "JSR" | "LEA" | "PEA"
    ) {
        return Ok(None);
    }
    let regs = case.initial.registers()?;
    let expected = case.expected.registers()?;
    let op = case.initial.prefetch[0];
    let base = regs.pc.wrapping_add(2);
    let mut push = None;
    match case.cohort.as_str() {
        "Bcc" | "BSR" => {
            let word = op & 255 == 0;
            let delta = if word {
                i32::from(case.initial.prefetch[1] as i16)
            } else {
                i32::from(op as u8 as i8)
            };
            let following = base.wrapping_add(if word { 2 } else { 0 });
            let taken = case.cohort == "BSR" || condition((op >> 8) & 15, regs.status_register());
            let pc = if taken {
                base.wrapping_add_signed(delta)
            } else {
                following
            };
            if expected.pc != pc {
                return Err("branch target contradicts initial state".into());
            }
            if case.cohort == "BSR" {
                push = Some(following);
            }
        }
        "DBcc" => {
            let register = usize::from(op & 7);
            let taken_condition = condition((op >> 8) & 15, regs.status_register());
            let low = (regs.d[register] as u16).wrapping_sub(1);
            let destination = if taken_condition {
                regs.d[register]
            } else {
                (regs.d[register] & 0xffff0000) | u32::from(low)
            };
            let pc = if !taken_condition && low != 65535 {
                base.wrapping_add_signed(i32::from(case.initial.prefetch[1] as i16))
            } else {
                base.wrapping_add(2)
            };
            if expected.d[register] != destination || expected.pc != pc {
                return Err("DBcc count or target contradicts initial state".into());
            }
        }
        "Scc" => {}
        _ => {
            let (target, following) = address(case, &regs)?;
            if matches!(case.cohort.as_str(), "JMP" | "JSR") {
                if expected.pc != target {
                    return Err("control target contradicts initial state".into());
                }
            } else {
                if expected.pc != following {
                    return Err("control cursor contradicts initial state".into());
                }
                if case.cohort == "LEA" {
                    let destination = usize::from((op >> 9) & 7);
                    let actual = if destination == 7 {
                        expected.stack_pointer()
                    } else {
                        expected.a[destination]
                    };
                    if actual != target {
                        return Err("LEA address contradicts initial state".into());
                    }
                }
            }
            if case.cohort == "JSR" {
                push = Some(following);
            }
            if case.cohort == "PEA" {
                push = Some(target);
            }
        }
    }
    let writes: Vec<_> = case
        .transactions
        .iter()
        .filter_map(|phase| {
            if let Phase::Write(value) = phase {
                Some(value)
            } else {
                None
            }
        })
        .collect();
    let Some(value) = push else {
        if case.cohort != "Scc" && !writes.is_empty() {
            return Err("fabricated control stack writes".into());
        }
        return Ok(None);
    };
    let addresses = [4, 2].map(|offset| regs.stack_pointer().wrapping_sub(offset) & 0x00ff_ffff);
    let values = [(value >> 16) as u16, value as u16];
    let function_code = if regs.supervisor() { 5 } else { 1 };
    if writes.len() != 2
        || writes.iter().zip(addresses.into_iter().zip(values)).any(
            |(transfer, (address, data))| {
                transfer.address_bus != address
                    || transfer.data_bus != u32::from(data)
                    || transfer.function_code != function_code
                    || transfer.uds != 1
                    || transfer.lds != 1
            },
        )
    {
        return Err("control stack frame contradicts opcode and initial state".into());
    }
    if expected.stack_pointer() != regs.stack_pointer().wrapping_sub(4) {
        return Err("control final stack differs".into());
    }
    Ok(Some(StackContract {
        addresses,
        function_code,
    }))
}
