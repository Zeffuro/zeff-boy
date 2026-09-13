use super::{
    Budget, MAX_ROM_BYTES, NsqNativeProfile, ReadError, ReadResult, RomSpan, ScanStop, data, half,
    signatures, span, wod, word,
};

const ROM_BASE: u32 = 0x0800_0000;
const MAX_PROFILES: usize = 16;

struct BankCall {
    directory: String,
    path: String,
    directory_span: RomSpan,
    path_span: RomSpan,
    arguments: Vec<[u32; 2]>,
    spans: Vec<RomSpan>,
}

pub(super) fn recognize(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<NsqNativeProfile>, ScanStop> {
    if bytes.len() > MAX_ROM_BYTES {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for at in (0..bytes.len().saturating_sub(31)).step_by(4) {
        budget.charge()?;
        if word(bytes, at) != Some(0x4657_b5f0) {
            continue;
        }
        for layout in signatures::LAYOUTS {
            if !layout
                .offsets
                .iter()
                .zip(&layout.patterns)
                .all(|(&offset, pattern)| signatures::matches(bytes, at + offset, pattern))
            {
                continue;
            }
            match inspect(bytes, at, layout, budget) {
                Ok(found) => {
                    if profiles.len() + found.len() > MAX_PROFILES {
                        return Err(ScanStop::InventoryLimit);
                    }
                    profiles.extend(found);
                }
                Err(ReadError::Invalid) => {}
                Err(ReadError::Stop(stop)) => return Err(stop),
            }
        }
    }
    Ok(profiles)
}

fn inspect(
    bytes: &[u8],
    bank: usize,
    layout: &signatures::Layout,
    budget: &mut Budget<'_>,
) -> ReadResult<Vec<NsqNativeProfile>> {
    let entries = layout.offsets.map(|offset| bank + offset);
    let mut setup = Vec::new();
    for &entry in &entries {
        setup.push(span(bytes, entry, 32)?);
    }
    let release_load = bank + layout.release_load;
    if !signatures::matches(bytes, release_load, &layout.release_copy) {
        return Err(ReadError::Invalid);
    }
    let release_address = literal(bytes, release_load, &mut setup)?;
    let release_offset = release_address
        .checked_sub(ROM_BASE)
        .ok_or(ReadError::Invalid)? as usize;
    let (release_name, release_prefix) = wod::path(bytes, release_offset)?;
    if release_name != "audio\\patches\\midi" {
        return Err(ReadError::Invalid);
    }
    setup.extend([span(bytes, release_load, 6)?, release_prefix]);
    let state = literal(bytes, entries[3] + layout.vblank_state, &mut setup)?;
    if !ram(state) || literal(bytes, entries[4] + layout.mix_state, &mut setup)? != state {
        return Err(ReadError::Invalid);
    }
    for &entry in &entries[..5] {
        for offset in (0..32).step_by(2) {
            if half(bytes, entry + offset).is_some_and(|value| value & 0xf800 == 0x4800) {
                let value = literal(bytes, entry + offset, &mut setup)?;
                if !ram(value) && !(0x0400_0000..0x0400_0400).contains(&value) {
                    return Err(ReadError::Invalid);
                }
            }
        }
    }
    let mut profiles = Vec::new();
    for call in callers(bytes, bank, budget)? {
        let filesystems = wod::find(bytes, &call.path, budget)?;
        for filesystem in filesystems {
            let tables = data::scan_tables(bytes, filesystem, budget)?;
            for song_table in tables {
                if profiles.len() == MAX_PROFILES {
                    return Err(ScanStop::InventoryLimit.into());
                }
                let mut setup_spans = setup.clone();
                setup_spans.extend_from_slice(&call.spans);
                setup_spans.sort_unstable();
                setup_spans.dedup();
                profiles.push(NsqNativeProfile {
                    load_bank: RomSpan::new(entries[0], 32),
                    load_songs: RomSpan::new(entries[1], 32),
                    play: RomSpan::new(entries[2], 32),
                    vblank: RomSpan::new(entries[3], 32),
                    mix: RomSpan::new(entries[4], 32),
                    song_table,
                    filesystem,
                    bank_directory: call.directory_span,
                    instrument_path: call.path_span,
                    release_prefix,
                    bank_arguments: call.arguments.clone(),
                    setup_spans,
                });
            }
        }
    }
    Ok(profiles)
}

fn callers(bytes: &[u8], bank: usize, budget: &mut Budget<'_>) -> ReadResult<Vec<BankCall>> {
    let mut result: Vec<BankCall> = Vec::new();
    for at in (4..bytes.len().saturating_sub(3)).step_by(2) {
        budget.charge()?;
        if branch_target(bytes, at) != Some(bank)
            || half(bytes, at - 4).is_none_or(|v| v & 0xff00 != 0x4800)
            || half(bytes, at - 2).is_none_or(|v| v & 0xff00 != 0x4900)
        {
            continue;
        }
        let mut spans = vec![span(bytes, at - 4, 8)?];
        let directory_address = literal(bytes, at - 4, &mut spans)?;
        let path_address = literal(bytes, at - 2, &mut spans)?;
        let Some(directory_offset) = directory_address.checked_sub(ROM_BASE) else {
            continue;
        };
        let Some(path_offset) = path_address.checked_sub(ROM_BASE) else {
            continue;
        };
        let (directory, directory_span) = match wod::path(bytes, directory_offset as usize) {
            Ok(value) => value,
            Err(ReadError::Invalid) => continue,
            Err(error) => return Err(error),
        };
        let (path, path_span) = match wod::path(bytes, path_offset as usize) {
            Ok(value) => value,
            Err(ReadError::Invalid) => continue,
            Err(error) => return Err(error),
        };
        if !path.to_ascii_lowercase().ends_with(".npf") {
            continue;
        }
        spans.extend([directory_span, path_span]);
        let arguments = [directory_address, path_address];
        if let Some(call) = result.iter_mut().find(|call| {
            call.directory.eq_ignore_ascii_case(&directory) && call.path.eq_ignore_ascii_case(&path)
        }) {
            if !call.arguments.contains(&arguments) {
                if call.arguments.len() == MAX_PROFILES {
                    return Err(ScanStop::InventoryLimit.into());
                }
                call.arguments.push(arguments);
                call.spans.extend(spans);
            }
        } else {
            if result.len() == MAX_PROFILES {
                return Err(ScanStop::InventoryLimit.into());
            }
            result.push(BankCall {
                directory,
                path,
                directory_span,
                path_span,
                arguments: vec![arguments],
                spans,
            });
        }
    }
    Ok(result)
}

fn literal(bytes: &[u8], at: usize, setup: &mut Vec<RomSpan>) -> ReadResult<u32> {
    let instruction = half(bytes, at).ok_or(ReadError::Invalid)?;
    if instruction & 0xf800 != 0x4800 {
        return Err(ReadError::Invalid);
    }
    let slot = ((at + 4) & !3) + usize::from(instruction & 255) * 4;
    setup.push(span(bytes, slot, 4)?);
    word(bytes, slot).ok_or(ReadError::Invalid)
}

fn ram(value: u32) -> bool {
    (0x0200_0000..0x0204_0000).contains(&value) || (0x0300_0000..0x0300_7e00).contains(&value)
}

fn branch_target(bytes: &[u8], at: usize) -> Option<usize> {
    let high = half(bytes, at)?;
    let low = half(bytes, at + 2)?;
    if high & 0xf800 != 0xf000 || low & 0xf800 != 0xf800 {
        return None;
    }
    let relative =
        ((((u32::from(high) & 0x7ff) << 12) | ((u32::from(low) & 0x7ff) << 1)) as i32) << 9 >> 9;
    let target = at as i64 + 4 + i64::from(relative);
    (target >= 0 && target < bytes.len() as i64).then_some(target as usize)
}
