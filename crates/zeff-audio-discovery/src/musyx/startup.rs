use super::{Budget, RomSpan, ScanStop, branch_target, half, literal, signatures, thumb_bl, word};

const ROM_BASE: u32 = 0x0800_0000;
const MAIN_LEN: usize = 0x1ec;
const RAM_LOADER: &[u32] = &[
    0xe92d_40ff,
    0xe1a0_400f,
    0xe284_4e15,
    0xe3a0_5403,
    0xeb00_0001,
    0xe3a0_0403,
    0xe12f_ff10,
    0xe92d_4000,
];
const DISPLAY_LOADER: &[u32] = &[
    0xe3a0_1301,
    0xe3a0_0c01,
    0xe1c1_00b8,
    0xe3a0_0000,
    0xe581_0000,
    0xe3a0_b405,
    0xe1df_c3b0,
    0xe59f_0028,
    0xe3a0_1402,
    0xeb00_000a,
    0xe3a0_b405,
    0xe3a0_1000,
    0xe1cb_10b0,
    0xe3a0_1301,
    0xe3a0_0080,
    0xe1c1_00b0,
    0xe59f_0000,
    0xe12f_ff10,
];
const PALETTE_LOADER: &[u32] = &[
    0x0096_3231,
    0x0000_0000,
    0x0000_0000,
    0x0000_f000,
    0xe3a0_0405,
    0xe3e0_1000,
    0xe1a0_2001,
    0xe1a0_3001,
    0xe1a0_4001,
    0xe3a0_5020,
    0xe8a0_001e,
    0xe8a0_001e,
    0xe255_5001,
    0x1aff_fffb,
    0xe59f_0074,
];
const THUMB_INTRO_LOADER: &[u32] = &[
    0xe59f_d03c,
    0xe28f_0001,
    0xe12f_ff10,
    0xa147_480e,
    0xe029_4a0b,
    0x46fe_490d,
    0x490b_4708,
    0xa007_4a0c,
    0x431a_4b0c,
    0xc307_4b0c,
    0x4a0d_490c,
    0x4b09_a003,
    0x4b09_431a,
    0x490b_c307,
    0x0000_4708,
    0x0000_0000,
    0x0000_3ad8,
    0x0300_7f00,
    0x0200_0000,
    0x0200_0001,
    0x0000_8000,
    0x8500_0000,
    0x0400_00d4,
    0x0300_0000,
    0x0000_1fc0,
    0x0800_00c0,
];
const PACKED_INTRO_LOADER: &[u32] = &[
    0xeb00_0015,
    0xe59f_008c,
    0xeb00_001a,
    0xe3a0_1403,
    0xe28f_e000,
    0xe1a0_f000,
    0xe59f_007c,
    0xeb00_0015,
    0xe59f_107c,
    0xe28f_e000,
    0xe3a0_f403,
    0xe28f_0044,
    0xe28f_e000,
    0xe59f_f068,
    0xe3a0_0059,
    0xef01_0000,
    0xef05_0000,
    0xeb00_0004,
    0xe3a0_0082,
    0xef01_0000,
    0xe59f_0048,
    0xeb00_0007,
    0xe590_f000,
    0xe3a0_0405,
    0xe3e0_1000,
    0xe3a0_5c01,
    0xe480_1004,
    0xe255_5001,
    0x1aff_fffc,
    0xe12f_ff1e,
    0xe28f_1028,
    0xe8b1_000c,
    0xe170_0002,
    0x1081_1002,
    0x1081_1003,
    0x1aff_fffa,
    0xe1a0_0001,
    0xe12f_ff1e,
];
const MAIN_PREFIX: &[u16] = &[
    0xb570, 0xb08d, 0x4900, 0x4a00, 0x1c10, 0x8008, 0x2500, 0x950b, 0x4c00, 0xa90b, 0x6021, 0x2080,
    0x0480, 0x6060, 0x4800, 0x60a0, 0x68a0, 0x950b, 0x6021, 0x20c0, 0x0480, 0x6060, 0x4800, 0x60a0,
    0x68a0, 0xa90c, 0x2600, 0x800d, 0x6021, 0x20c0, 0x04c0, 0x6060, 0x4800, 0x60a0, 0x68a0, 0x800d,
    0x6021, 0x20a0, 0x04c0, 0x6060, 0x4800, 0x60a0, 0x68a0, 0xf000, 0xf800, 0x4800, 0x8801, 0x200f,
    0x4388, 0x280f, 0xd102, 0x20ff, 0xf000, 0xf800, 0x4800, 0x6020, 0x4900, 0x6061, 0x4800, 0x60a0,
    0x68a0, 0x4800, 0x6020, 0x4800, 0x6060, 0x4800, 0x60a0, 0x68a0, 0x4800, 0x6001, 0x4800, 0x6005,
    0x4800, 0x7006,
];
const CONFIGURE: &[u16] = &[
    0x4669, 0x4800, 0x8008, 0x4668, 0x70c6, 0x2002, 0x70c8, 0x2006, 0x70c8, 0x2016, 0x70c8, 0x2008,
    0x7088, 0x4668, 0x8085, 0x4800, 0x9002, 0xac07, 0x4668, 0x1c21, 0xf000, 0xf800, 0x2180, 0x0189,
    0x1c22, 0x4288, 0xd93d,
];
const CALL_INIT: &[u16] = &[
    0x4900, 0xac03, 0x1c10, 0x1c22, 0xf000, 0xf800, 0x4a00, 0x4b00, 0x4668, 0x1c21, 0xf000, 0xf800,
];

pub(super) struct Setup {
    pub boot_entry: u32,
    pub configuration_entry: Option<RomSpan>,
    pub spans: Vec<RomSpan>,
}

pub(super) fn recover(
    bytes: &[u8],
    image: usize,
    init: usize,
    boot_entry: u32,
    budget: &mut Budget<'_>,
) -> Result<Option<Setup>, ScanStop> {
    let Some(boot) = boot_entry.checked_sub(ROM_BASE).map(|value| value as usize) else {
        return Ok(None);
    };
    let Some(mut spans) = original_crt(bytes, image) else {
        return Ok(None);
    };
    if boot > init
        && let Some(pattern) = [
            DISPLAY_LOADER,
            PALETTE_LOADER,
            THUMB_INTRO_LOADER,
            PACKED_INTRO_LOADER,
        ]
        .into_iter()
        .find(|pattern| arm_matches(bytes, boot, pattern))
    {
        spans.push(RomSpan::new(boot, pattern.len() * 4));
        return Ok(Some(Setup {
            boot_entry: ROM_BASE + image as u32 + 0xc0,
            configuration_entry: None,
            spans,
        }));
    }
    let crt = image + 0xc0;
    if boot != crt
        || word(bytes, crt + 0x28) != Some(0xe1a0_e00f)
        || word(bytes, crt + 0x2c) != Some(0xe12f_ff11)
    {
        return Ok(None);
    }
    let Some((launcher, pointer)) = arm_literal(bytes, crt + 0x24, 1) else {
        return Ok(None);
    };
    let Some(launcher) = launcher.checked_sub(ROM_BASE).map(|value| value as usize) else {
        return Ok(None);
    };
    if launcher <= init || !arm_matches(bytes, launcher, RAM_LOADER) {
        return Ok(None);
    }
    let mut found = None;
    for main in ((crt + 0x30)..init.min(image + 0x1000)).step_by(2) {
        budget.charge()?;
        let Some(witnesses) = configuration_main(bytes, main, init) else {
            continue;
        };
        if found.is_some() {
            return Ok(None);
        }
        found = Some((main, witnesses));
    }
    let Some((main, witnesses)) = found else {
        return Ok(None);
    };
    spans.extend(witnesses);
    spans.extend([pointer, RomSpan::new(launcher, RAM_LOADER.len() * 4)]);
    Ok(Some(Setup {
        boot_entry,
        configuration_entry: Some(RomSpan::new(main, MAIN_LEN)),
        spans,
    }))
}

fn original_crt(bytes: &[u8], image: usize) -> Option<Vec<RomSpan>> {
    let original = image.checked_add(0xc0)?;
    let entry = if word(bytes, original)? >> 24 == 0xea {
        branch_target(bytes, original)?
    } else {
        original
    };
    if entry < original
        || entry > original + 0x100
        || !entry.is_multiple_of(4)
        || !matches!(word(bytes, entry)?, 0xe3a0_0012 | 0xe3a0_00d2)
        || word(bytes, entry + 4)? != 0xe129_f000
        || word(bytes, entry + 12)? != 0xe3a0_001f
        || word(bytes, entry + 16)? != 0xe129_f000
    {
        return None;
    }
    let mut spans = vec![RomSpan::new(original, entry - original + 24)];
    for instruction in [entry + 8, entry + 20] {
        let (stack, span) = arm_literal(bytes, instruction, 13)?;
        if !(0x0300_0000..=0x0300_7fa0).contains(&stack) || !stack.is_multiple_of(4) {
            return None;
        }
        spans.push(span);
    }
    Some(spans)
}

fn configuration_main(bytes: &[u8], main: usize, init: usize) -> Option<Vec<RomSpan>> {
    bytes.get(main..main.checked_add(MAIN_LEN)?)?;
    if !thumb_matches(bytes, main, MAIN_PREFIX)
        || !thumb_matches(bytes, main + 0xc6, CONFIGURE)
        || !thumb_matches(bytes, main + 0x178, CALL_INIT)
        || thumb_bl(bytes, main + 0x18c)? != init
    {
        return None;
    }
    let mut spans = vec![RomSpan::new(main, MAIN_LEN)];
    for (call, patterns) in [
        (main + 0xee, signatures::ESTIMATE),
        (main + 0x180, signatures::ASSIGN),
    ] {
        let target = thumb_bl(bytes, call)?;
        if !patterns
            .iter()
            .any(|signature| signature.matches(bytes, target))
        {
            return None;
        }
        bytes.get(target..target.checked_add(64)?)?;
        spans.push(RomSpan::new(target, 64));
    }
    // This entry performs its own RAM initialization before constructing the native settings.
    for (offset, expected) in [
        (0x10, 0x0400_00d4),
        (0x1c, 0x8501_0000),
        (0x2c, 0x8500_1f80),
        (0x40, 0x8100_c000),
        (0x50, 0x8100_0200),
        (0x178, 0x0300_0030),
    ] {
        let (value, witness) = literal(bytes, main + offset)?;
        if value != expected {
            return None;
        }
        spans.push(witness);
    }
    for (source_at, destination_at, control_at) in [(0x6c, 0x70, 0x74), (0x7a, 0x7e, 0x82)] {
        let (source, source_span) = literal(bytes, main + source_at)?;
        let (destination, destination_span) = literal(bytes, main + destination_at)?;
        let (control, control_span) = literal(bytes, main + control_at)?;
        if control >> 16 != 0x8000 || control as u16 == 0 {
            return None;
        }
        let length = usize::from(control as u16) * 2;
        let source = source.checked_sub(ROM_BASE)? as usize;
        bytes.get(source..source.checked_add(length)?)?;
        let destination_end = destination.checked_add(length as u32)?;
        if !source.is_multiple_of(2)
            || !destination.is_multiple_of(2)
            || !((0x0200_0000..=0x0204_0000).contains(&destination)
                && destination_end <= 0x0204_0000
                || (0x0300_0000..0x0300_7e00).contains(&destination)
                    && destination_end <= 0x0300_7e00)
        {
            return None;
        }
        spans.extend([
            source_span,
            destination_span,
            control_span,
            RomSpan::new(source, length),
        ]);
    }
    let (root, root_span) = literal(bytes, main + 0xe4)?;
    let root = root.checked_sub(ROM_BASE)? as usize;
    bytes.get(root..root.checked_add(32)?)?;
    spans.push(root_span);
    Some(spans)
}

fn arm_literal(bytes: &[u8], at: usize, register: u32) -> Option<(u32, RomSpan)> {
    let instruction = word(bytes, at)?;
    if instruction & 0xffff_f000 != 0xe59f_0000 | register << 12 {
        return None;
    }
    let address = at.checked_add(8 + (instruction & 0xfff) as usize)?;
    Some((word(bytes, address)?, RomSpan::new(address, 4)))
}

fn arm_matches(bytes: &[u8], at: usize, pattern: &[u32]) -> bool {
    pattern
        .iter()
        .enumerate()
        .all(|(i, &expected)| word(bytes, at + i * 4) == Some(expected))
}

fn thumb_matches(bytes: &[u8], at: usize, pattern: &[u16]) -> bool {
    pattern.iter().enumerate().all(|(i, &expected)| {
        let mask = match expected & 0xf800 {
            0x4800 => 0xff00,
            0xf000 | 0xf800 => 0xf800,
            _ => 0xffff,
        };
        half(bytes, at + i * 2).is_ok_and(|actual| actual & mask == expected & mask)
    })
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
