use super::*;
use zeff_m68k_verify::reference::Phase;

fn word(case: &Case, address: u32) -> Result<u16, String> {
    let byte = |at| {
        case.initial
            .ram
            .iter()
            .find(|(address, _)| *address == at)
            .map(|(_, value)| u16::from(*value))
            .ok_or("missing CMPI code RAM")
    };
    let address = address & 0x00ff_ffff;
    Ok(byte(address)? << 8 | byte(address.wrapping_add(1) & 0x00ff_ffff)?)
}

pub(super) fn profile(case: &Case, initial: Registers) -> Result<Profile, String> {
    let opcode = case.initial.prefetch[0];
    let mode = opcode >> 3 & 7;
    let register = usize::from(opcode & 7);
    let long = opcode >> 6 & 3 == 2;
    let size = if long { 4 } else { 2 };
    let shifted = initial.pc.wrapping_add(size);
    let mut prefix = vec![Prefix::Read(initial.pc.wrapping_add(4))];
    if long {
        prefix.push(Prefix::Read(initial.pc.wrapping_add(6)));
    }
    if matches!(mode, 4 | 6) {
        prefix.push(Prefix::Idle(2));
    }
    let extension = if mode >= 5 {
        prefix.push(Prefix::Read(shifted.wrapping_add(4)));
        word(case, shifted.wrapping_add(2))?
    } else {
        0
    };
    let base = initial.address_register(register).unwrap();
    let operand = match (mode, register) {
        (2 | 3, _) => base,
        (4, _) => base.wrapping_sub(size),
        (5, _) => base.wrapping_add_signed(i32::from(extension as i16)),
        (6, _) => {
            let selector = usize::from(extension >> 12 & 7);
            let raw = if extension & 0x8000 != 0 {
                initial.address_register(selector).unwrap()
            } else {
                initial.d[selector]
            };
            let index = if extension & 0x0800 == 0 {
                raw as i16 as i32 as u32
            } else {
                raw
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(extension as u8 as i8))
        }
        (7, 0) => extension as i16 as i32 as u32,
        (7, 1) => {
            let low = word(case, shifted.wrapping_add(4))?;
            prefix.push(Prefix::Read(shifted.wrapping_add(6)));
            u32::from(extension) << 16 | u32::from(low)
        }
        _ => return Err("unselected CMPI memory operand".into()),
    };
    let mut phase = initial;
    if mode == 4 {
        phase.set_address_register(register, operand);
    } else if mode == 3 && !long {
        phase.set_address_register(register, base.wrapping_add(2));
    }
    let captured_pc = shifted.wrapping_add(match (mode, register) {
        (7, 1) => 6,
        (7, 0) => 4,
        (4, _) if !long => 4,
        _ => 2,
    });
    let fc = if initial.supervisor() { 5 } else { 1 };
    let frame = [
        captured_pc as u16,
        phase.status_register(),
        (captured_pc >> 16) as u16,
        opcode,
        operand as u16,
        (opcode & 0xffe0) | 0x10 | fc,
        (operand >> 16) as u16,
    ];
    for (index, expected) in frame.into_iter().enumerate() {
        if !matches!(case.transactions.get(prefix.len() + 2 + index),
            Some(Phase::Write(t)) if t.data_bus == u32::from(expected))
        {
            return Err("CMPI captured PC/SR/IRD/EA/SSW differs".into());
        }
    }
    let prefix_clocks = prefix
        .iter()
        .map(|p| match p {
            Prefix::Idle(n) => u64::from(*n),
            _ => 4,
        })
        .sum();
    Ok(Profile {
        initial,
        phase,
        operand,
        direction: Direction::Read,
        marker_word: None,
        marker: prefix.len(),
        prefix_clocks,
        prefix,
        operand_program: false,
        capture_known: true,
    })
}
