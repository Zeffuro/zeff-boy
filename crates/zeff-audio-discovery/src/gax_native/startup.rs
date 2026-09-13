use super::{Budget, GaxRamCopy, RomSpan, ScanStop, half, word};

pub(super) struct Setup {
    pub copies: Vec<GaxRamCopy>,
    pub spans: Vec<RomSpan>,
}

pub(super) fn images(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<usize>, ScanStop> {
    let mut result = vec![0];
    let Some(logo) = bytes.get(4..0xa0) else {
        return Ok(result);
    };
    for image in (4..bytes.len().saturating_sub(0xbf)).step_by(4) {
        if image % 32 == 4 {
            budget.charge()?;
        }
        if bytes[image + 3] != 0xea
            || bytes[image + 0xb2] != 0x96
            || bytes.get(image + 4..image + 0xa0) != Some(logo)
            || bytes[image + 0xa0..image + 0xbe]
                .iter()
                .fold(0x19u8, |sum, value| sum.wrapping_add(*value))
                != 0
            || !has_crt(bytes, image)
        {
            continue;
        }
        if result.len() == 128 {
            return Err(ScanStop::InventoryLimit);
        }
        result.push(image);
    }
    Ok(result)
}

pub(super) fn image_start(images: &[usize], at: usize) -> usize {
    images[images
        .partition_point(|&image| image <= at)
        .saturating_sub(1)]
}

fn has_crt(bytes: &[u8], image: usize) -> bool {
    let mut entry = image + 0xc0;
    if word(bytes, entry) == Some(0xea00_0000) {
        entry += 8;
    }
    if word(bytes, entry) == Some(0xe3a0_10ff) {
        entry += 4;
    }
    if !matches!(word(bytes, entry), Some(0xe3a0_0012 | 0xe3a0_00d2))
        || word(bytes, entry + 4) != Some(0xe129_f000)
        || word(bytes, entry + 12) != Some(0xe3a0_001f)
        || word(bytes, entry + 16) != Some(0xe129_f000)
    {
        return false;
    }
    [entry + 8, entry + 20].into_iter().all(|at| {
        arm_literal(bytes, at, 13).is_some_and(|(stack, _)| {
            (0x0300_0000..=0x0300_8000).contains(&stack) && stack.is_multiple_of(4)
        })
    })
}

pub(super) fn inspect(
    bytes: &[u8],
    image: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<Setup>, ScanStop> {
    if !has_crt(bytes, image) {
        return Ok(None);
    }
    let mut found = None;
    for at in ((image + 0xc0)..bytes.len().min(image + 0x300)).step_by(4) {
        budget.charge()?;
        let Some(setup) = block_copy(bytes, at)
            .or_else(|| register_copy(bytes, at))
            .or_else(|| peripheral_copy(bytes, at))
        else {
            continue;
        };
        if found.is_some() {
            return Ok(None);
        }
        found = Some(setup);
    }
    Ok(found)
}

pub(super) fn division_target(bytes: &[u8], at: usize) -> Option<(Option<u32>, RomSpan)> {
    if half(bytes, at) == Some(0xdf06) && half(bytes, at + 2) == Some(0x46f7) {
        return Some((None, RomSpan::new(at, 4)));
    }
    for pattern in [
        [
            0x2900, 0xd041, 0xb410, 0x1c04, 0x404c, 0x46a4, 0x2301, 0x2200, 0x2900, 0xd500, 0x4249,
            0x2800, 0xd500, 0x4240, 0x4288, 0xd32c,
        ],
        [
            0x2900, 0xd034, 0x2301, 0x2200, 0xb410, 0x4288, 0xd32c, 0x2401, 0x0724, 0x42a1, 0xd204,
            0x4281, 0xd202, 0x0109, 0x011b, 0xe7f8,
        ],
    ] {
        if pattern
            .iter()
            .enumerate()
            .all(|(i, &expected)| half(bytes, at + i * 2) == Some(expected))
        {
            return Some((None, RomSpan::new(at, 32)));
        }
    }
    let start = at;
    let at = if half(bytes, at) == Some(0x2300) && half(bytes, at + 2) == Some(0x469c) {
        at + 4
    } else {
        at
    };
    if half(bytes, at)? & 0xff00 != 0x4a00 || half(bytes, at + 2)? != 0x4710 {
        return None;
    }
    let slot = ((at + 4) & !3) + usize::from(half(bytes, at)? & 255) * 4;
    let target = word(bytes, slot)?;
    if !(0x0300_0000..0x0300_7e00).contains(&target) || !target.is_multiple_of(4) {
        return None;
    }
    Some((
        Some(target),
        RomSpan::new(start, slot.checked_add(4)?.checked_sub(start)?),
    ))
}

pub(super) fn contains_division(bytes: &[u8], copies: &[GaxRamCopy], address: u32) -> bool {
    copies.iter().any(|copy| {
        let Some(relative) = address.checked_sub(copy.destination) else {
            return false;
        };
        if relative
            .checked_add(16)
            .is_none_or(|end| end > copy.source.byte_len)
        {
            return false;
        }
        let at = copy.source.effective_offset as usize + relative as usize;
        [
            [0xe211_3102, 0x4261_1000, 0xe033_c040, 0x2260_0000],
            [0xe071_2fa0, 0x20c0_0f81, 0xe0a3_3003, 0xe071_2f20],
        ]
        .iter()
        .any(|pattern| matches_arm(bytes, at, pattern))
    })
}

fn block_copy(bytes: &[u8], at: usize) -> Option<Setup> {
    let prefix = [
        0xe3a0_c301,
        0xe3a0_0014,
        0xe380_0901,
        0xe3a0_1f81,
        0xe18c_00b1,
        0xe28c_c0d4,
    ];
    if at < 24
        || !matches_arm(bytes, at - 24, &prefix)
        || !matches_arm(
            bytes,
            at + 12,
            &[
                0xe043_3001,
                0xe3a0_2321,
                0xe182_2143,
                0xe88c_0007,
                0xe080_0003,
            ],
        )
        || !matches_arm(
            bytes,
            at + 40,
            &[0xe043_3001, 0xe3a0_2321, 0xe182_2143, 0xe88c_0007],
        )
    {
        return None;
    }
    let mut spans = vec![RomSpan::new(at - 24, 80)];
    let source = load(bytes, at, 0, &mut spans)?;
    let first = load(bytes, at + 4, 1, &mut spans)?;
    let first_end = load(bytes, at + 8, 3, &mut spans)?;
    let second = load(bytes, at + 32, 1, &mut spans)?;
    let second_end = load(bytes, at + 36, 3, &mut spans)?;
    copies(bytes, source, first..first_end, second..second_end, spans)
}

fn register_copy(bytes: &[u8], at: usize) -> Option<Setup> {
    for (offset, expected) in [
        (8, 0xe582_0000),
        (16, 0xe582_0004),
        (24, 0xe041_3000),
        (28, 0xe3a0_1321),
        (32, 0xe181_0143),
        (36, 0xe582_0008),
        (48, 0xe080_0003),
        (52, 0xe582_0000),
        (60, 0xe582_0004),
        (68, 0xe041_0000),
        (72, 0xe3a0_1321),
        (76, 0xe181_0140),
        (80, 0xe582_0008),
    ] {
        if word(bytes, at + offset)? != expected {
            return None;
        }
    }
    let mut spans = vec![RomSpan::new(at, 84)];
    if load(bytes, at, 2, &mut spans)? != 0x0400_00d4
        || load(bytes, at + 40, 2, &mut spans)? != 0x0400_00d4
    {
        return None;
    }
    let source = load(bytes, at + 4, 0, &mut spans)?;
    if load(bytes, at + 44, 0, &mut spans)? != source {
        return None;
    }
    let first = load(bytes, at + 12, 0, &mut spans)?;
    let first_end = load(bytes, at + 20, 1, &mut spans)?;
    let second = load(bytes, at + 56, 0, &mut spans)?;
    let second_end = load(bytes, at + 64, 1, &mut spans)?;
    copies(bytes, source, first..first_end, second..second_end, spans)
}

fn peripheral_copy(bytes: &[u8], at: usize) -> Option<Setup> {
    if at < 20
        || !matches_arm(
            bytes,
            at - 20,
            &[
                0xe3a0_c301,
                0xe3a0_0901,
                0xe380_0014,
                0xe3a0_1f81,
                0xe18c_00b1,
            ],
        )
    {
        return None;
    }
    for (offset, expected) in [
        (4, 0xe58c_00d4),
        (12, 0xe58c_00d8),
        (20, 0xe041_3000),
        (24, 0xe3a0_1321),
        (28, 0xe181_0143),
        (32, 0xe58c_00dc),
        (40, 0xe080_0003),
        (44, 0xe58c_00d4),
        (52, 0xe58c_00d8),
        (60, 0xe041_0000),
        (64, 0xe3a0_1321),
        (68, 0xe181_0140),
        (72, 0xe58c_00dc),
    ] {
        if word(bytes, at + offset)? != expected {
            return None;
        }
    }
    let mut spans = vec![RomSpan::new(at - 20, 96)];
    let source = load(bytes, at, 0, &mut spans)?;
    if load(bytes, at + 36, 0, &mut spans)? != source {
        return None;
    }
    let first = load(bytes, at + 8, 0, &mut spans)?;
    let first_end = load(bytes, at + 16, 1, &mut spans)?;
    let second = load(bytes, at + 48, 0, &mut spans)?;
    let second_end = load(bytes, at + 56, 1, &mut spans)?;
    copies(bytes, source, first..first_end, second..second_end, spans)
}

fn copies(
    bytes: &[u8],
    source: u32,
    first: std::ops::Range<u32>,
    second: std::ops::Range<u32>,
    mut spans: Vec<RomSpan>,
) -> Option<Setup> {
    let first_length = first.end.checked_sub(first.start)?;
    let second_source = source.checked_add(first_length)?;
    let copies = vec![
        valid_copy(bytes, source, first.start, first_length)?,
        valid_copy(
            bytes,
            second_source,
            second.start,
            second.end.checked_sub(second.start)?,
        )?,
    ];
    if copies[0].destination < copies[1].destination + copies[1].source.byte_len
        && copies[1].destination < copies[0].destination + copies[0].source.byte_len
    {
        return None;
    }
    spans.extend(copies.iter().map(|copy| copy.source));
    Some(Setup { copies, spans })
}

fn valid_copy(bytes: &[u8], source: u32, destination: u32, length: u32) -> Option<GaxRamCopy> {
    let end = destination.checked_add(length)?;
    if length == 0
        || !length.is_multiple_of(4)
        || !destination.is_multiple_of(4)
        || !((0x0200_0000..0x0204_0000).contains(&destination) && end <= 0x0204_0000
            || (0x0300_0000..0x0300_7e00).contains(&destination) && end <= 0x0300_7e00)
    {
        return None;
    }
    let at = super::super::rom_pointer(bytes, source, length as usize, 4)?;
    Some(GaxRamCopy {
        source: RomSpan::new(at, length as usize),
        destination,
    })
}

fn matches_arm(bytes: &[u8], at: usize, pattern: &[u32]) -> bool {
    pattern
        .iter()
        .enumerate()
        .all(|(index, &expected)| word(bytes, at + index * 4) == Some(expected))
}

fn arm_literal(bytes: &[u8], at: usize, register: u32) -> Option<(u32, RomSpan)> {
    let instruction = word(bytes, at)?;
    if instruction & 0xffff_f000 != 0xe59f_0000 | register << 12 {
        return None;
    }
    let slot = at.checked_add(8 + (instruction & 0xfff) as usize)?;
    Some((word(bytes, slot)?, RomSpan::new(slot, 4)))
}

fn load(bytes: &[u8], at: usize, register: u32, spans: &mut Vec<RomSpan>) -> Option<u32> {
    let (value, span) = arm_literal(bytes, at, register)?;
    spans.push(span);
    Some(value)
}
