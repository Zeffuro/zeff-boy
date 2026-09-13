use super::super::{
    Budget, GaxNativeEntry, GaxNativeLayout, GaxNativeProfile, RomSpan, ScanStop, half, startup,
    word,
};

pub(super) const INIT: &[u16] = &[
    0xb5f0, 0x4657, 0x464e, 0x4645, 0xb4e0, 0xb081, 0x1c07, 0x2600, 0x480e, 0x6839, 0x6001, 0x6b39,
    0x4681, 0x480d, 0x4684, 0x2900, 0xd100, 0x6338, 0x6afd, 0x2d00, 0xd100, 0x823e, 0x8938, 0x4909,
    0x6b3b, 0x4288, 0xd101, 0x8b18, 0x8138, 0x8978, 0x4288, 0xd111,
];
pub(super) const IRQ: &[u16] = &[
    0xb5f0, 0x483b, 0x6802, 0x6811, 0x483a, 0x4281, 0xd16d, 0x6d50, 0x2800, 0xd06a, 0x6d50, 0x2801,
    0xd11a, 0x2002, 0x6550, 0x4936, 0x2080, 0x8008, 0x4835, 0x2500,
];
pub(super) const PLAY: &[u16] = &[
    0xb570, 0xb081, 0x4847, 0x6801, 0x6d48, 0x2800, 0xd100, 0xe000, 0x6848, 0x6b40, 0x2800, 0xd02b,
    0x1c04, 0x2640,
];

pub(super) struct Profile {
    pub image: usize,
    pub native: GaxNativeProfile,
    pub spans: Vec<RomSpan>,
    pub version_span: RomSpan,
}

pub(crate) fn is_init(bytes: &[u8], at: usize) -> bool {
    init_layout(bytes, at).is_some()
}

pub(super) fn recognize(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<(Vec<usize>, Vec<Profile>), ScanStop> {
    let versions = versions(bytes, budget)?;
    if versions.is_empty() {
        return Ok((vec![0], Vec::new()));
    }
    let images = startup::images(bytes, budget)?;
    let mut profiles = Vec::new();
    for init in (0..bytes.len().saturating_sub(64)).step_by(2) {
        if init % 32 == 0 {
            budget.charge()?;
        }
        let Some(layout) = init_layout(bytes, init) else {
            continue;
        };
        let image = startup::image_start(&images, init);
        let Some((state, state_span)) = literal(bytes, init + 0x10) else {
            continue;
        };
        if !state.is_multiple_of(4)
            || !matches!(state, 0x0200_0000..=0x0203_fffc | 0x0300_0000..=0x0300_7dfc)
        {
            continue;
        }
        let Some(new) = unique(
            bytes,
            image.max(init.saturating_sub(0x800))..init,
            budget,
            |at| constructor(bytes, at, layout),
        )?
        else {
            continue;
        };
        let Some(irq) = unique(
            bytes,
            (init + 64)..bytes.len().min(init + 0x2000),
            budget,
            |at| {
                exact(bytes, at, IRQ)
                    && literal(bytes, at + 2).is_some_and(|(value, _)| value == state)
            },
        )?
        else {
            continue;
        };
        let Some(play) = unique(
            bytes,
            (irq + 40)..bytes.len().min(irq + 0x400),
            budget,
            |at| {
                play_matches(bytes, at, layout)
                    && literal(bytes, at + 4).is_some_and(|(value, _)| value == state)
            },
        )?
        else {
            continue;
        };
        let mut spans = vec![
            RomSpan::new(new, 0xac),
            RomSpan::new(init, 64),
            RomSpan::new(irq, IRQ.len() * 2),
            RomSpan::new(play, PLAY.len() * 2),
            state_span,
        ];
        spans.extend([
            literal(bytes, irq + 2).unwrap().1,
            literal(bytes, play + 4).unwrap().1,
        ]);
        let mut ram_targets = Vec::new();
        let mut valid = true;
        for call in [init + 0x3da, init + 0x6ec] {
            let Some(target) = thumb_bl(bytes, call) else {
                valid = false;
                break;
            };
            let Some((ram, witness)) = startup::division_target(bytes, target) else {
                valid = false;
                break;
            };
            spans.extend([RomSpan::new(call, 4), witness]);
            ram_targets.extend(ram);
        }
        if !valid {
            continue;
        }
        let ram_copies = if ram_targets.is_empty() {
            Vec::new()
        } else {
            let Some(setup) = startup::inspect(bytes, image, budget)? else {
                continue;
            };
            if !ram_targets
                .iter()
                .all(|&target| startup::contains_division(bytes, &setup.copies, target))
            {
                continue;
            }
            spans.extend(setup.spans);
            setup.copies
        };
        let selected: Vec<_> = versions
            .iter()
            .filter(|(at, _)| startup::image_start(&images, *at) == image)
            .collect();
        if selected.is_empty() {
            continue;
        }
        let (version_at, first) = selected[0];
        let version = if selected.iter().all(|(_, version)| version == first) {
            first.clone()
        } else {
            "GAX Sound Engine 3 (multiple embedded revisions)".into()
        };
        for (at, version) in &selected {
            spans.push(RomSpan::new(*at, version.chars().count() + 1));
        }
        let entry = |at, length| GaxNativeEntry {
            source: RomSpan::new(at, length),
            cpu_address: 0x0800_0001 + at as u32,
        };
        let native = GaxNativeProfile {
            version,
            new: Some(entry(new, 0xac)),
            init: entry(init, 64),
            mix: entry(irq, IRQ.len() * 2),
            play: entry(play, PLAY.len() * 2),
            layout,
            work_ram: state,
            sample_rate: u16::MAX,
            ram_copies,
        };
        if super::driver::workspace(&native).is_none() {
            continue;
        }
        super::super::merge_spans(&mut spans);
        profiles.push(Profile {
            image,
            native,
            spans,
            version_span: RomSpan::new(*version_at, first.chars().count() + 1),
        });
        if profiles.len() == 32 {
            return Err(ScanStop::InventoryLimit);
        }
    }
    Ok((images, profiles))
}

fn init_layout(bytes: &[u8], at: usize) -> Option<GaxNativeLayout> {
    let modern = match half(bytes, at + 22)? {
        0x6b39 => false,
        0x6b79 => true,
        _ => return None,
    };
    INIT.iter()
        .enumerate()
        .all(|(i, &expected)| {
            let expected = if modern && matches!(i, 11 | 17 | 18 | 24) {
                expected + 0x40
            } else {
                expected
            };
            half(bytes, at + 2 * i) == Some(expected)
        })
        .then_some(if modern {
            GaxNativeLayout::V3Modern
        } else {
            GaxNativeLayout::V3Legacy
        })
}

fn constructor(bytes: &[u8], at: usize, layout: GaxNativeLayout) -> bool {
    let size = if layout == GaxNativeLayout::V3Modern {
        0x40
    } else {
        0x3c
    };
    bytes.get(at..at + 0xac).is_some()
        && exact(
            bytes,
            at,
            &[
                0xb5f0, 0x4647, 0xb480, 0xb081, 0x1c06, 0x2e00, 0xd108, 0x4802, 0x4902,
            ],
        )
        && thumb_bl(bytes, at + 18).is_some()
        && exact(
            bytes,
            at + 0x20,
            &[
                0x1c34,
                0x2700 | size,
                0x2003,
                0x4030,
                0x2100 | (size - 4),
                0x1989,
                0x4688,
            ],
        )
        && exact(
            bytes,
            at + 0x80,
            &[
                0x8130, 0x2001, 0x4240, 0x8170, 0x2000, 0x81b0, 0x3801, 0x8230, 0x8270, 0x2001,
                0x4641, 0x7008,
            ],
        )
}

fn play_matches(bytes: &[u8], at: usize, layout: GaxNativeLayout) -> bool {
    PLAY.iter().enumerate().all(|(i, &expected)| {
        let expected = if i == 9 && layout == GaxNativeLayout::V3Modern {
            0x6b80
        } else {
            expected
        };
        let mask = if i == 7 { 0xf800 } else { 0xffff };
        half(bytes, at + i * 2).is_some_and(|actual| actual & mask == expected & mask)
    })
}

fn unique(
    bytes: &[u8],
    range: std::ops::Range<usize>,
    budget: &mut Budget<'_>,
    predicate: impl Fn(usize) -> bool,
) -> Result<Option<usize>, ScanStop> {
    let mut found = None;
    for at in range.step_by(2) {
        budget.charge()?;
        if at < bytes.len() && predicate(at) && found.replace(at).is_some() {
            return Ok(None);
        }
    }
    Ok(found)
}

fn literal(bytes: &[u8], at: usize) -> Option<(u32, RomSpan)> {
    let instruction = half(bytes, at)?;
    if instruction & 0xf800 != 0x4800 {
        return None;
    }
    let slot = ((at + 4) & !3) + usize::from(instruction & 255) * 4;
    Some((word(bytes, slot)?, RomSpan::new(slot, 4)))
}

fn exact(bytes: &[u8], at: usize, pattern: &[u16]) -> bool {
    pattern
        .iter()
        .enumerate()
        .all(|(i, &expected)| half(bytes, at + 2 * i) == Some(expected))
}

pub(super) fn thumb_bl(bytes: &[u8], at: usize) -> Option<usize> {
    let first = half(bytes, at)?;
    let second = half(bytes, at + 2)?;
    if first & 0xf800 != 0xf000 || second & 0xf800 != 0xf800 {
        return None;
    }
    let high = (i32::from(first & 0x7ff) << 21) >> 9;
    usize::try_from(at as i64 + 4 + i64::from(high) + i64::from(second & 0x7ff) * 2).ok()
}

fn versions(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<(usize, String)>, ScanStop> {
    let prefix = b"GAX Sound Engine ";
    let mut result = Vec::new();
    for at in 0..bytes.len().saturating_sub(prefix.len()) {
        if at % 16 == 0 {
            budget.charge()?;
        }
        if !bytes[at..].starts_with(prefix) {
            continue;
        }
        let tail = &bytes[at..bytes.len().min(at + 128)];
        let Some(end) = tail.iter().position(|&value| value == 0) else {
            continue;
        };
        let version = if matches!(tail.get(prefix.len()), Some(b'v' | b'V')) {
            prefix.len() + 1
        } else {
            prefix.len()
        };
        if tail.get(version..version + 2) != Some(b"3.") || !tail[..end].contains(&0xa9) {
            continue;
        }
        if result.len() == 16 {
            return Err(ScanStop::InventoryLimit);
        }
        result.push((at, tail[..end].iter().copied().map(char::from).collect()));
    }
    Ok(result)
}
