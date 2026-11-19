use super::plan::{Memory, ReadKind, Step, read};
use zeff_m68k::Registers;

fn index(registers: Registers, extension: u16) -> u32 {
    let register = usize::from(extension >> 12 & 7);
    let value = if extension & 0x8000 == 0 {
        registers.d[register]
    } else {
        registers.address_register(register).unwrap()
    };
    if extension & 0x0800 == 0 {
        (value as i16 as i32) as u32
    } else {
        value
    }
}

pub(super) struct Source {
    pub value: u32,
    pub retained: u16,
    pub anchor: u32,
    pub address: Option<u32>,
}

pub(super) fn operand(
    size: u16,
    queue: [u16; 2],
    initial: Registers,
    expected: &mut Registers,
    memory: &Memory,
    steps: &mut Vec<Step>,
) -> Result<Source, String> {
    let opcode = queue[0];
    let mode = opcode >> 3 & 7;
    let reg = usize::from(opcode & 7);
    let mut retained = queue[1];
    let mut anchor = initial.pc.wrapping_add(2);
    let mut address = None;
    let value = match mode {
        0 => initial.d[reg],
        1 => initial.address_register(reg).unwrap(),
        7 if reg == 4 => {
            let high = retained;
            anchor = anchor.wrapping_add(2);
            let first = read(steps, memory, anchor, initial, ReadKind::Prefetch)?;
            if size == 2 {
                anchor = anchor.wrapping_add(2);
                retained = read(steps, memory, anchor, initial, ReadKind::Prefetch)?;
                u32::from(high) << 16 | u32::from(first)
            } else {
                retained = first;
                u32::from(high)
            }
        }
        _ => {
            if size != 0 {
                return Err("remaining memory source must be Byte".into());
            }
            let base = initial.address_register(reg).unwrap();
            let increment = if reg == 7 { 2 } else { 1 };
            let extension = retained;
            if matches!(mode, 4 | 6) || mode == 7 && reg == 3 {
                steps.push(Step::Idle(2));
            }
            let ea = match mode {
                2 | 3 => base,
                4 => base.wrapping_sub(increment),
                5 => base.wrapping_add_signed(i32::from(extension as i16)),
                6 => base
                    .wrapping_add(index(initial, extension))
                    .wrapping_add_signed(i32::from(extension as u8 as i8)),
                7 => match reg {
                    0 => (extension as i16 as i32) as u32,
                    1 => {
                        anchor = anchor.wrapping_add(2);
                        let low = read(steps, memory, anchor, initial, ReadKind::Prefetch)?;
                        u32::from(extension) << 16 | u32::from(low)
                    }
                    2 => initial
                        .pc
                        .wrapping_add(2)
                        .wrapping_add_signed(i32::from(extension as i16)),
                    3 => initial
                        .pc
                        .wrapping_add(2)
                        .wrapping_add(index(initial, extension))
                        .wrapping_add_signed(i32::from(extension as u8 as i8)),
                    _ => return Err("remaining memory EA differs".into()),
                },
                _ => return Err("remaining source mode differs".into()),
            };
            if mode >= 5 {
                anchor = anchor.wrapping_add(2);
                retained = read(steps, memory, anchor, initial, ReadKind::Prefetch)?;
            }
            if mode == 3 {
                expected.set_address_register(reg, base.wrapping_add(increment));
            } else if mode == 4 {
                expected.set_address_register(reg, ea);
            }
            address = Some(ea);
            u32::from(read(
                steps,
                memory,
                ea,
                initial,
                if mode == 7 && reg >= 2 {
                    ReadKind::ProgramByte
                } else {
                    ReadKind::DataByte
                },
            )?)
        }
    };
    Ok(Source {
        value,
        retained,
        anchor,
        address,
    })
}
