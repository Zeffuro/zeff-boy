use super::{Budget, MusyxNativeProfile, RomSpan, ScanStop, half, signatures, word};

#[path = "startup.rs"]
mod startup;

pub(super) struct Signature {
    pub value: &'static [u16],
    pub mask: &'static [u16],
}

impl Signature {
    fn matches(&self, bytes: &[u8], at: usize) -> bool {
        self.value
            .iter()
            .zip(self.mask)
            .enumerate()
            .all(|(i, (&value, &mask))| {
                half(bytes, at + i * 2).is_ok_and(|actual| actual & mask == value & mask)
            })
    }
}

pub(super) struct Inventory {
    pub profiles: Vec<MusyxNativeProfile>,
    images: Vec<usize>,
}

impl Inventory {
    pub fn image_start(&self, offset: usize) -> usize {
        let index = self.images.partition_point(|&image| image <= offset);
        self.images[index.saturating_sub(1)]
    }
}

pub(super) fn recognize(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Inventory, ScanStop> {
    let inventory = Inventory {
        profiles: Vec::new(),
        images: images(bytes, budget)?,
    };
    let mut result = Vec::new();
    for init in (0..bytes.len().saturating_sub(80)).step_by(4) {
        if init % 32 == 0 {
            budget.charge()?;
        }
        if !signatures::INIT
            .iter()
            .any(|signature| signature.matches(bytes, init))
        {
            continue;
        }
        let compact_init = half(bytes, init).ok() == Some(0xb570);
        let state_at = init + if compact_init { 0x34 } else { 0x48 };
        let Some((state_pointer, state_span)) = literal(bytes, state_at) else {
            continue;
        };
        if !state_pointer.is_multiple_of(4)
            || !matches!(state_pointer,
            0x0200_0000..=0x0203_fffc | 0x0300_0000..=0x0300_7edc)
        {
            continue;
        }
        let mut functions = Vec::new();
        let mut setup_spans = vec![RomSpan::new(init, 80), state_span];
        for (patterns, state_offset) in [
            (signatures::SELECT, 12),
            (signatures::START, 0),
            (signatures::UPDATE, 50),
            (signatures::IRQ, 12),
        ] {
            let mut matches = Vec::new();
            for at in ((init + 12)..(init + 0x10000).min(bytes.len().saturating_sub(64))).step_by(2)
            {
                budget.charge()?;
                if !patterns
                    .iter()
                    .any(|signature| signature.matches(bytes, at))
                {
                    continue;
                }
                let delta = if state_offset == 0 && half(bytes, at).ok() == Some(0xb500) {
                    2
                } else {
                    state_offset
                };
                if let Some((value, witness)) = literal(bytes, at + delta)
                    && value == state_pointer
                {
                    matches.push((at, witness));
                }
            }
            if let [(at, witness)] = matches.as_slice() {
                functions.push(RomSpan::new(*at, 64));
                setup_spans.extend([RomSpan::new(*at, 64), *witness]);
            }
        }
        let [select, start, update, timer1_irq] = functions.as_slice() else {
            continue;
        };
        let image = inventory.image_start(init);
        let Some((mut boot_entry, boot_span)) = boot_entry(bytes, image, &inventory) else {
            continue;
        };
        setup_spans.push(boot_span);
        let mut configuration_entry = configuration_entry(bytes, init, image, budget)?;
        if configuration_entry.is_none()
            && let Some(setup) = startup::recover(bytes, image, init, boot_entry, budget)?
        {
            boot_entry = setup.boot_entry;
            configuration_entry = setup.configuration_entry;
            setup_spans.extend(setup.spans);
        }
        if let Some(entry) = configuration_entry {
            setup_spans.push(entry);
        }
        setup_spans.sort_unstable();
        setup_spans.dedup();
        result.push(MusyxNativeProfile {
            init: RomSpan::new(init, 80),
            select: *select,
            start: *start,
            update: *update,
            timer1_irq: *timer1_irq,
            state_pointer,
            compact_init,
            boot_entry,
            configuration_entry,
            setup_spans,
        });
        if result.len() >= 32 {
            return Err(ScanStop::InventoryLimit);
        }
    }
    Ok(Inventory {
        profiles: result,
        ..inventory
    })
}

fn images(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<usize>, ScanStop> {
    let mut result = vec![0];
    let Some(logo) = bytes.get(4..0xa0) else {
        return Ok(result);
    };
    for at in (4..bytes.len().saturating_sub(0xbf)).step_by(4) {
        if at % 32 == 4 {
            budget.charge()?;
        }
        if bytes[at + 3] != 0xea
            || bytes[at + 0xb2] != 0x96
            || bytes.get(at + 4..at + 0xa0) != Some(logo)
        {
            continue;
        }
        let checksum = bytes[at + 0xa0..at + 0xbe]
            .iter()
            .fold(0x19u8, |sum, value| sum.wrapping_add(*value));
        if checksum != 0 {
            continue;
        }
        let Some(entry) = branch_target(bytes, at) else {
            continue;
        };
        // A copied cartridge header alone does not identify another executable image.
        if entry < at + 0xc0
            || !matches!(word(bytes, entry), Some(0xe3a0_0012 | 0xe3a0_00d2))
            || word(bytes, entry + 4) != Some(0xe129_f000)
            || word(bytes, entry + 12) != Some(0xe3a0_001f)
            || word(bytes, entry + 16) != Some(0xe129_f000)
        {
            continue;
        }
        if ![entry + 8, entry + 20].into_iter().all(|instruction| {
            word(bytes, instruction).is_some_and(|value| {
                value & 0xffff_f000 == 0xe59f_d000
                    && word(bytes, instruction + 8 + (value & 0xfff) as usize).is_some_and(
                        |stack| {
                            (0x0300_0000..=0x0300_8000).contains(&stack) && stack.is_multiple_of(4)
                        },
                    )
            })
        }) {
            continue;
        }
        if result.len() >= 128 {
            return Err(ScanStop::InventoryLimit);
        }
        result.push(at);
    }
    Ok(result)
}

fn branch_target(bytes: &[u8], at: usize) -> Option<usize> {
    let branch = word(bytes, at)?;
    if branch >> 24 != 0xea {
        return None;
    }
    let relative = ((branch << 8) as i32 >> 6) as i64;
    let entry = usize::try_from(at as i64 + 8 + relative).ok()?;
    bytes.get(entry..entry.checked_add(32)?)?;
    Some(entry)
}

fn boot_entry(bytes: &[u8], image: usize, inventory: &Inventory) -> Option<(u32, RomSpan)> {
    let mut entry = branch_target(bytes, image)?;
    let original = entry;
    // Some multi-cart headers redirect to another image before their own CRT setup.
    if word(bytes, entry)? & 0xffff_f000 == 0xe59f_0000
        && word(bytes, entry + 4)? == 0xe12f_ff10
        && word(bytes, entry + 8)? == 0xe3a0_0012
        && word(bytes, entry + 12)? == 0xe129_f000
    {
        let target = word(bytes, entry + 8 + (word(bytes, entry)? & 0xfff) as usize)?;
        let target_offset = target.checked_sub(0x0800_0000)? as usize;
        if target_offset < bytes.len() && inventory.image_start(target_offset) != image {
            entry += 8;
        }
    }
    Some((0x0800_0000 + entry as u32, RomSpan::new(original, 32)))
}

fn configuration_entry(
    bytes: &[u8],
    init: usize,
    image: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<RomSpan>, ScanStop> {
    let prefix: &[u16] = &[
        0xb530, 0xb08b, 0x4b00, 0x6818, 0x1c42, 0x601a, 0x2a01, 0xdc00, 0x4900, 0x2001, 0x7008,
        0x4900, 0x2000, 0x6008, 0x1c50, 0x6018,
    ];
    let mut found = None;
    for at in (image..init).step_by(2) {
        budget.charge()?;
        if !prefix.iter().enumerate().all(|(i, &expected)| {
            let mask = if matches!(i, 2 | 7 | 8 | 11) {
                0xff00
            } else {
                0xffff
            };
            half(bytes, at + 2 * i).is_ok_and(|actual| actual & mask == expected & mask)
        }) {
            continue;
        }
        if thumb_bl(bytes, at + 0x66) != Some(init)
            || !thumb_bl(bytes, at + 0x42).is_some_and(|target| {
                signatures::ESTIMATE
                    .iter()
                    .any(|s| s.matches(bytes, target))
            })
            || !thumb_bl(bytes, at + 0x5a)
                .is_some_and(|target| signatures::ASSIGN.iter().any(|s| s.matches(bytes, target)))
            || literal(bytes, at + 0x38)
                .is_none_or(|(root, _)| !(0x0800_0000..0x0a00_0000).contains(&root))
            || bytes.get(at..at + 0xa8).is_none()
        {
            continue;
        }
        if found.is_some() {
            return Ok(None);
        }
        found = Some(RomSpan::new(at, 0xa8));
    }
    Ok(found)
}

fn literal(bytes: &[u8], at: usize) -> Option<(u32, RomSpan)> {
    let instruction = half(bytes, at).ok()?;
    if instruction & 0xf800 != 0x4800 {
        return None;
    }
    let address = ((at + 4) & !3) + usize::from(instruction & 255) * 4;
    Some((word(bytes, address)?, RomSpan::new(address, 4)))
}

fn thumb_bl(bytes: &[u8], at: usize) -> Option<usize> {
    let first = half(bytes, at).ok()?;
    let second = half(bytes, at + 2).ok()?;
    if first & 0xf800 != 0xf000 || second & 0xf800 != 0xf800 {
        return None;
    }
    let high = (i32::from(first & 0x7ff) << 21) >> 9;
    usize::try_from(at as i64 + 4 + i64::from(high) + i64::from(second & 0x7ff) * 2).ok()
}
