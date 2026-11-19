use super::*;
use zeff_m68k_verify::reference::Phase;

pub(super) fn profile(case: &Case, initial: Registers) -> Result<Profile, String> {
    let opcode = case.initial.prefetch[0];
    let mode = opcode >> 3 & 7;
    let register = usize::from(opcode & 7);
    let extension = case.initial.prefetch[1];
    let program = mode == 7 && matches!(register, 2 | 3);
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
        (4, _) => base.wrapping_sub(2),
        (5, _) | (7, 2) => base.wrapping_add_signed(i32::from(extension as i16)),
        (6, _) | (7, 3) => {
            let selector = usize::from(extension >> 12 & 7);
            let value = if extension & 0x8000 != 0 {
                initial.address_register(selector).unwrap()
            } else {
                initial.d[selector]
            };
            let index = if extension & 0x0800 != 0 {
                value
            } else {
                value as i16 as i32 as u32
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(extension as u8 as i8))
        }
        (7, 0) => extension as i16 as i32 as u32,
        (7, 1) => {
            let byte = |at: u32| {
                case.initial
                    .ram
                    .iter()
                    .find(|(address, _)| *address == at & 0x00ff_ffff)
                    .map(|(_, value)| u32::from(*value))
                    .ok_or("missing status absolute-long RAM")
            };
            let at = initial.pc.wrapping_add(4);
            let low = byte(at)? << 8 | byte(at.wrapping_add(1))?;
            prefix.push(Prefix::Read(initial.pc.wrapping_add(6)));
            u32::from(extension) << 16 | low
        }
        _ => return Err("unselected status source".into()),
    };
    let mut phase = initial;
    if mode == 3 {
        phase.set_address_register(register, base.wrapping_add(2));
    }
    if mode == 4 {
        phase.set_address_register(register, operand);
    }
    let captured_pc = initial.pc.wrapping_add(match (mode, register) {
        (7, 1) => 6,
        (7, 0) | (4, _) => 4,
        _ => 2,
    });
    let fc = match (initial.supervisor(), program) {
        (false, false) => 1,
        (false, true) => 2,
        (true, false) => 5,
        (true, true) => 6,
    };
    let frame = [
        captured_pc as u16,
        initial.status_register(),
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
            return Err("status captured PC/SR/IRD/EA/SSW differs".into());
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
        operand_program: program,
        capture_known: true,
    })
}
