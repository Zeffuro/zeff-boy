use super::{Prefix, Profile, known};
use zeff_m68k::Registers;
use zeff_m68k::bus::Direction;
use zeff_m68k_verify::reference::Case;

fn word(case: &Case, address: u32) -> Result<u16, String> {
    let address = address & 0x00ff_fffe;
    let byte = |at| {
        case.initial
            .ram
            .iter()
            .find(|(address, _)| *address == at)
            .map(|(_, value)| u16::from(*value))
            .ok_or_else(|| format!("missing immediate profile RAM at {at:06x}"))
    };
    Ok(byte(address)? << 8 | byte(address + 1)?)
}

pub(super) fn profile(case: &Case, initial: Registers) -> Result<Profile, String> {
    let opcode = case.initial.prefetch[0];
    let mode = (opcode >> 6) & 7;
    let register = usize::from((opcode >> 9) & 7);
    if !matches!(opcode >> 12, 2 | 3) || !(matches!(mode, 2..=6) || mode == 7 && register <= 1) {
        return Err("unsupported immediate address-fault route".into());
    }
    let long = opcode >> 12 == 2;
    let mut prefix = vec![Prefix::Read(initial.pc.wrapping_add(4))];
    let value = if long {
        prefix.push(Prefix::Read(initial.pc.wrapping_add(6)));
        u32::from(case.initial.prefetch[1]) << 16
            | u32::from(word(case, initial.pc.wrapping_add(4))?)
    } else {
        u32::from(case.initial.prefetch[1])
    };
    let anchor = initial.pc.wrapping_add(if long { 6 } else { 4 });
    let retained = word(case, anchor)?;
    let base = initial.address_register(register).unwrap();
    let operand = match mode {
        2 | 3 => base,
        4 => base.wrapping_sub(2),
        5 => base.wrapping_add_signed(i32::from(retained as i16)),
        6 => {
            prefix.push(Prefix::Idle(2));
            let index_register = usize::from((retained >> 12) & 7);
            let index = if retained & 0x8000 == 0 {
                initial.d[index_register]
            } else {
                initial.address_register(index_register).unwrap()
            };
            let index = if retained & 0x0800 == 0 {
                i32::from(index as i16) as u32
            } else {
                index
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(retained as u8 as i8))
        }
        7 if register == 0 => i32::from(retained as i16) as u32,
        7 => u32::from(retained) << 16 | u32::from(word(case, anchor.wrapping_add(2))?),
        _ => unreachable!(),
    };
    if mode >= 5 {
        prefix.push(Prefix::Read(anchor.wrapping_add(2)));
        if mode == 7 && register == 1 {
            prefix.push(Prefix::Read(anchor.wrapping_add(4)));
        }
    } else if mode == 4 {
        prefix.push(Prefix::Read(anchor.wrapping_add(2)));
    }
    let mut phase = initial;
    if mode == 4 && !long {
        phase.set_address_register(register, operand);
    }
    let sr = initial.status_register();
    let flags = if !long {
        Some(
            (sr & !15)
                | if value == 0 {
                    4
                } else if value & 32768 != 0 {
                    8
                } else {
                    0
                },
        )
    } else if mode == 2 || mode == 3 {
        None
    } else if mode == 5 || mode == 6 {
        Some(
            (sr & !12)
                | if value & 0x8000_0000 != 0 { 8 } else { 0 }
                | if value >> 16 == 0 { sr & 4 } else { 0 },
        )
    } else {
        Some(
            (sr & !15)
                | if value == 0 {
                    4
                } else if value & 0x8000_0000 != 0 {
                    8
                } else {
                    0
                },
        )
    };
    if let Some(sr) = flags {
        phase.set_status_register(sr);
    }
    let prefix_clocks = prefix
        .iter()
        .map(|p| match p {
            Prefix::Idle(clocks) => u64::from(*clocks),
            Prefix::Read(_) | Prefix::DataRead(_, _) => 4,
        })
        .sum();
    Ok(Profile {
        initial,
        phase,
        operand,
        direction: Direction::Write,
        marker_word: Some(if !long || mode == 4 {
            value as u16
        } else {
            (value >> 16) as u16
        }),
        marker: prefix.len(),
        prefix_clocks,
        prefix,
        operand_program: false,
        capture_known: mode != 4 || long || known(retained),
    })
}
