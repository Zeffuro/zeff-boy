use super::{Prefix, Profile, known};
use zeff_m68k::Registers;
use zeff_m68k::bus::{BUS_ADDRESS_MASK, Direction};
use zeff_m68k_verify::reference::Case;

fn word(case: &Case, address: u32) -> Result<u16, String> {
    let address = address & BUS_ADDRESS_MASK & !1;
    let byte = |at| {
        case.initial
            .ram
            .iter()
            .find(|(address, _)| *address == at)
            .map(|(_, value)| u16::from(*value))
            .ok_or_else(|| format!("missing memory profile RAM at {at:06x}"))
    };
    Ok(byte(address)? << 8 | byte(address + 1)?)
}

fn flags(sr: u16, value: u32, long: bool) -> u16 {
    let value = if long { value } else { value & 65535 };
    (sr & !15)
        | if value == 0 { 4 } else { 0 }
        | if value & if long { 0x8000_0000 } else { 32768 } != 0 {
            8
        } else {
            0
        }
}

pub(super) fn profile(case: &Case, initial: Registers) -> Result<Profile, String> {
    let opcode = case.initial.prefetch[0];
    let source = (opcode >> 3) & 7;
    let source_register = usize::from(opcode & 7);
    let destination = (opcode >> 6) & 7;
    let destination_register = usize::from((opcode >> 9) & 7);
    if !matches!(opcode >> 12, 2 | 3)
        || !(matches!(destination, 2..=6) || destination == 7 && destination_register <= 1)
        || !(matches!(source, 2..=6) || source == 7 && source_register <= 3)
    {
        return Err("unsupported memory address-fault route".into());
    }
    let long = opcode >> 12 == 2;
    let stride = if long { 4 } else { 2 };
    let program = source == 7 && source_register >= 2;
    let indexed = source == 6 || source == 7 && source_register == 3;
    let mut prefix = Vec::new();
    if source == 4 || indexed {
        prefix.push(Prefix::Idle(2));
    }
    let extensions = if source == 7 && source_register == 1 {
        2
    } else {
        usize::from(source >= 5)
    };
    if extensions > 0 {
        prefix.push(Prefix::Read(initial.pc.wrapping_add(4)));
    }
    if extensions == 2 {
        prefix.push(Prefix::Read(initial.pc.wrapping_add(6)));
    }
    let extension = case.initial.prefetch[1];
    let base = if program {
        initial.pc.wrapping_add(2)
    } else {
        initial.address_register(source_register).unwrap()
    };
    let operand = match (source, source_register) {
        (2 | 3, _) => base,
        (4, _) => base.wrapping_sub(stride),
        (5, _) | (7, 2) => base.wrapping_add_signed(i32::from(extension as i16)),
        (6, _) | (7, 3) => {
            let reg = usize::from((extension >> 12) & 7);
            let index = if extension & 0x8000 == 0 {
                initial.d[reg]
            } else {
                initial.address_register(reg).unwrap()
            };
            let index = if extension & 0x0800 == 0 {
                i32::from(index as i16) as u32
            } else {
                index
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(extension as u8 as i8))
        }
        (7, 0) => i32::from(extension as i16) as u32,
        (7, 1) => u32::from(extension) << 16 | u32::from(word(case, initial.pc.wrapping_add(4))?),
        _ => unreachable!(),
    };
    let mut phase = initial;
    if source == 4 || source == 3 && !long {
        phase.set_address_register(
            source_register,
            if source == 4 {
                operand
            } else {
                base.wrapping_add(stride)
            },
        );
    }
    if operand & 1 != 0 {
        let prefix_clocks = clocks(&prefix);
        return Ok(Profile {
            initial,
            phase,
            operand,
            direction: Direction::Read,
            marker_word: None,
            marker: prefix.len(),
            prefix_clocks,
            prefix,
            operand_program: program,
            capture_known: true,
        });
    }
    prefix.push(Prefix::DataRead(operand, program));
    let high = word(case, operand)?;
    let value = if long {
        prefix.push(Prefix::DataRead(operand.wrapping_add(2), program));
        if source == 3 {
            phase.set_address_register(source_register, base.wrapping_add(stride));
        }
        u32::from(high) << 16 | u32::from(word(case, operand.wrapping_add(2))?)
    } else {
        u32::from(high)
    };
    let anchor = initial.pc.wrapping_add(2 + extensions as u32 * 2);
    let next = if extensions == 0 {
        extension
    } else {
        word(case, anchor)?
    };
    let destination_base = phase.address_register(destination_register).unwrap();
    let operand = match destination {
        4 => destination_base.wrapping_sub(2),
        5 => destination_base.wrapping_add_signed(i32::from(next as i16)),
        6 => {
            prefix.push(Prefix::Idle(2));
            let reg = usize::from((next >> 12) & 7);
            let index = if next & 0x8000 == 0 {
                phase.d[reg]
            } else {
                phase.address_register(reg).unwrap()
            };
            let index = if next & 0x0800 == 0 {
                i32::from(index as i16) as u32
            } else {
                index
            };
            destination_base
                .wrapping_add(index)
                .wrapping_add_signed(i32::from(next as u8 as i8))
        }
        7 if destination_register == 0 => i32::from(next as i16) as u32,
        7 => u32::from(next) << 16 | u32::from(word(case, anchor.wrapping_add(2))?),
        _ => destination_base,
    };
    if destination >= 5 {
        prefix.push(Prefix::Read(anchor.wrapping_add(2)));
    }
    if destination == 4 {
        prefix.push(Prefix::Read(anchor.wrapping_add(2)));
        if !long {
            phase.set_address_register(destination_register, operand);
        }
    }
    phase.set_status_register(flags(
        initial.status_register(),
        value,
        long && (matches!(destination, 4..=6) || destination == 7 && destination_register == 0),
    ));
    let prefix_clocks = clocks(&prefix);
    Ok(Profile {
        initial,
        phase,
        operand,
        direction: Direction::Write,
        marker_word: Some(if !long || destination == 4 {
            value as u16
        } else {
            (value >> 16) as u16
        }),
        marker: prefix.len(),
        prefix_clocks,
        prefix,
        operand_program: false,
        capture_known: destination != 4 || long || known(next),
    })
}

fn clocks(prefix: &[Prefix]) -> u64 {
    prefix
        .iter()
        .map(|p| match p {
            Prefix::Idle(n) => u64::from(*n),
            Prefix::Read(_) | Prefix::DataRead(_, _) => 4,
        })
        .sum()
}
