use super::*;

pub(super) fn profile(case: &Case, initial: Registers) -> Result<Profile, String> {
    let opcode = case.initial.prefetch[0];
    let mode = opcode >> 3 & 7;
    let register = usize::from(opcode & 7);
    let long = matches!(opcode >> 6 & 7, 2 | 7);
    let extension = case.initial.prefetch[1];
    let program = mode == 7 && register >= 2;
    let base = if program {
        initial.pc.wrapping_add(2)
    } else {
        initial.address_register(register).unwrap()
    };
    let mut prefix = Vec::new();
    if matches!(mode, 4 | 6) || mode == 7 && register == 3 {
        prefix.push(Prefix::Idle(2));
    }
    if mode >= 5 {
        prefix.push(Prefix::Read(initial.pc.wrapping_add(4)));
    }
    let operand = match (mode, register) {
        (2 | 3, _) => base,
        (4, _) => base.wrapping_sub(if long { 4 } else { 2 }),
        (5, _) | (7, 2) => base.wrapping_add_signed(i32::from(extension as i16)),
        (6, _) | (7, 3) => {
            let index = usize::from(extension >> 12 & 7);
            let value = if extension & 0x8000 != 0 {
                initial.address_register(index).unwrap()
            } else {
                initial.d[index]
            };
            let value = if extension & 0x0800 == 0 {
                value as i16 as i32 as u32
            } else {
                value
            };
            base.wrapping_add(value)
                .wrapping_add_signed(i32::from(extension as u8 as i8))
        }
        (7, 0) => extension as i16 as i32 as u32,
        (7, 1) => {
            let address = initial.pc.wrapping_add(4) & 0x00ff_fffe;
            let byte = |at| {
                case.initial
                    .ram
                    .iter()
                    .find(|(address, _)| *address == at)
                    .map(|(_, value)| u32::from(*value))
                    .ok_or("missing CMP absolute-long RAM")
            };
            prefix.push(Prefix::Read(initial.pc.wrapping_add(6)));
            u32::from(extension) << 16 | byte(address)? << 8 | byte(address + 1)?
        }
        _ => return Err("unselected CMP source".into()),
    };
    let mut phase = initial;
    if mode == 4 {
        phase.set_address_register(register, operand);
    }
    if mode == 3 && !long {
        phase.set_address_register(register, base.wrapping_add(2));
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
        operand_program: program,
        capture_known: true,
    })
}
