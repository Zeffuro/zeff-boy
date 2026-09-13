use super::{Budget, GaxNativeEntry, RomSpan, ScanStop, half, startup};

pub(super) fn inspect(
    bytes: &[u8],
    init: GaxNativeEntry,
    budget: &mut Budget<'_>,
) -> Result<Option<startup::Setup>, ScanStop> {
    let at = init.source.effective_offset as usize;
    let mut setup = startup::Setup {
        copies: Vec::new(),
        spans: Vec::new(),
    };
    if !bytes[at..].starts_with(super::INIT_SIGNATURES[2].bytes) {
        return Ok(Some(setup));
    }
    if let Some(setup) = rom_division_setup(bytes, at, budget)? {
        return Ok(Some(setup));
    }
    let mut targets = Vec::new();
    for offset in [0x2bc, 0x31c, 0x58c] {
        budget.charge()?;
        let Some(target) = thumb_bl(bytes, at + offset) else {
            return Ok(None);
        };
        let Some((ram, witness)) = startup::division_target(bytes, target) else {
            return Ok(None);
        };
        setup.spans.extend([RomSpan::new(at + offset, 4), witness]);
        targets.extend(ram);
    }
    if !targets.is_empty() {
        let images = startup::images(bytes, budget)?;
        let image = startup::image_start(&images, at);
        let Some(copied) = startup::inspect(bytes, image, budget)? else {
            return Ok(None);
        };
        if !targets
            .iter()
            .all(|&target| startup::contains_division(bytes, &copied.copies, target))
        {
            return Ok(None);
        }
        setup.spans.extend(copied.spans);
        setup.copies = copied.copies;
    }
    Ok(Some(setup))
}

fn rom_division_setup(
    bytes: &[u8],
    init: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<startup::Setup>, ScanStop> {
    let start = init.checked_add(0x200).ok_or(ScanStop::WorkLimit)?;
    let end = init
        .checked_add(0x700)
        .ok_or(ScanStop::WorkLimit)?
        .min(bytes.len());
    let mut calls = Vec::new();
    for call in (start..end).step_by(2) {
        budget.charge()?;
        let Some(target) = thumb_bl(bytes, call) else {
            continue;
        };
        let Some((ram, body)) = startup::division_target(bytes, target) else {
            continue;
        };
        if ram.is_some() {
            return Ok(None);
        }
        calls.push((call, body));
    }
    if calls.len() != 3 || calls[0].1 != calls[1].1 || calls[1].1 == calls[2].1 {
        return Ok(None);
    }
    let spans = calls
        .into_iter()
        .flat_map(|(call, body)| [RomSpan::new(call, 4), body])
        .collect();
    Ok(Some(startup::Setup {
        copies: Vec::new(),
        spans,
    }))
}

fn thumb_bl(bytes: &[u8], at: usize) -> Option<usize> {
    let first = half(bytes, at)?;
    let second = half(bytes, at + 2)?;
    if first & 0xf800 != 0xf000 || second & 0xf800 != 0xf800 {
        return None;
    }
    let high = (i32::from(first & 0x7ff) << 21) >> 9;
    usize::try_from(at as i64 + 4 + i64::from(high) + i64::from(second & 0x7ff) * 2).ok()
}

#[cfg(test)]
mod tests;
