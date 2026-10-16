use super::*;

pub(super) fn profile(case: &Case, initial: Registers) -> Result<Profile, String> {
    let opcode = case.initial.prefetch[0];
    let mode = opcode >> 3 & 7;
    let register = usize::from(opcode & 7);
    let long = opcode >> 6 & 3 == 2;
    let extension = case.initial.prefetch[1];
    let base = initial.address_register(register).unwrap();
    let mut prefix = Vec::new();
    if matches!(mode, 4 | 6) {
        prefix.push(Prefix::Idle(2));
    }
    if mode >= 5 {
        prefix.push(Prefix::Read(initial.pc.wrapping_add(4)));
    }
    let operand = match (mode, register) {
        (2 | 3, _) => base,
        (4, _) => base.wrapping_sub(if long { 4 } else { 2 }),
        (5, _) => base.wrapping_add_signed(i32::from(extension as i16)),
        (6, _) => {
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
            let byte = |address| {
                case.initial
                    .ram
                    .iter()
                    .find(|(at, _)| *at == address)
                    .map(|(_, value)| u32::from(*value))
                    .ok_or("missing TST absolute-long RAM")
            };
            prefix.push(Prefix::Read(initial.pc.wrapping_add(6)));
            u32::from(extension) << 16 | byte(address)? << 8 | byte(address + 1)?
        }
        _ => return Err("unselected TST source".into()),
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
        operand_program: false,
        capture_known: true,
    })
}
