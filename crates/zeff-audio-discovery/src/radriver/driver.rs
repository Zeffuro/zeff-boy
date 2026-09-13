use super::{
    Budget, MAX_ROM_BYTES, MAX_TABLE_ENTRIES, RadriverNativeProfile, ReadError, ReadResult,
    RomSpan, ScanStop, half, pointer, signatures as sig, span, word,
};

mod context;
mod global;

const MAX_PROFILES: usize = 8;
const PROBE_GROUP_BYTES: usize = 256;

pub(super) fn recognize(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<(Vec<RadriverNativeProfile>, bool), ScanStop> {
    let mut profiles = Vec::new();
    let mut unresolved = false;
    if bytes.len() > MAX_ROM_BYTES {
        return Ok((profiles, unresolved));
    }
    for at in (0..bytes.len().saturating_sub(63)).step_by(4) {
        if at.is_multiple_of(PROBE_GROUP_BYTES) {
            budget.charge()?;
        }
        let found = if half(bytes, at) == Some(0xb530) && sig::matches(bytes, at, sig::GLOBAL_SONG)
        {
            budget.charge()?;
            global::inspect(bytes, at, budget)
        } else if half(bytes, at) == Some(0xb500)
            && half(bytes, at + 2) == Some(0x1c02)
            && half(bytes, at + 4).is_some_and(|op| op & 0xff00 == 0x2a00)
            && sig::matches(bytes, at + 6, &sig::CONTEXT_SONG[3..])
        {
            budget.charge()?;
            context::inspect(bytes, at, budget)
        } else {
            continue;
        };
        match found {
            Ok(profile) => {
                if profiles.len() == MAX_PROFILES {
                    return Err(ScanStop::InventoryLimit);
                }
                profiles.push(profile);
            }
            Err(ReadError::Invalid) => {}
            Err(ReadError::UnboundStartup) => unresolved = true,
            Err(ReadError::Stop(stop)) => return Err(stop),
        }
    }
    Ok((profiles, unresolved))
}

fn literal(bytes: &[u8], at: usize) -> ReadResult<u32> {
    let op = half(bytes, at).ok_or(ReadError::Invalid)?;
    if op & 0xf800 != 0x4800 {
        return Err(ReadError::Invalid);
    }
    word(bytes, ((at + 4) & !3) + usize::from(op & 255) * 4).ok_or(ReadError::Invalid)
}

fn literal_span(bytes: &[u8], at: usize) -> ReadResult<RomSpan> {
    let op = half(bytes, at).ok_or(ReadError::Invalid)?;
    if op & 0xf800 != 0x4800 {
        return Err(ReadError::Invalid);
    }
    span(bytes, ((at + 4) & !3) + usize::from(op & 255) * 4, 4)
}

fn branch(bytes: &[u8], at: usize) -> Option<usize> {
    let high = half(bytes, at)?;
    let low = half(bytes, at + 2)?;
    if high & 0xf800 != 0xf000 || low & 0xf800 != 0xf800 {
        return None;
    }
    let relative = (((high & 0x7ff) as i32) << 12) | (((low & 0x7ff) as i32) << 1);
    let relative = (relative << 9) >> 9;
    let target = (at as i64 + 4 + i64::from(relative)).try_into().ok()?;
    (target < bytes.len()).then_some(target)
}

fn ram(address: u32) -> bool {
    address.is_multiple_of(4) && (0x0300_0000..=0x0300_7b00).contains(&address)
}

fn bank(bytes: &[u8], address: u32) -> ReadResult<RomSpan> {
    let header = pointer(bytes, address, 16)?;
    let at = header.effective_offset as usize;
    let sequences = word(bytes, at).ok_or(ReadError::Invalid)? as usize;
    let count = word(bytes, at + 4).ok_or(ReadError::Invalid)? as usize;
    if count == 0 || count > MAX_TABLE_ENTRIES || sequences > MAX_TABLE_ENTRIES {
        return Err(ReadError::Invalid);
    }
    pointer(
        bytes,
        word(bytes, at + 12).ok_or(ReadError::Invalid)?,
        count * 4,
    )?;
    if sequences != 0 {
        pointer(
            bytes,
            word(bytes, at + 8).ok_or(ReadError::Invalid)?,
            sequences * 8,
        )?;
    }
    Ok(header)
}

fn find_one(
    bytes: &[u8],
    start: usize,
    end: usize,
    pattern: &[u16],
    budget: &mut Budget<'_>,
) -> ReadResult<usize> {
    let mut result = None;
    for at in (start..end.min(bytes.len())).step_by(2) {
        budget.charge()?;
        if sig::matches(bytes, at, pattern) {
            if result.is_some() {
                return Err(ReadError::Invalid);
            }
            result = Some(at);
        }
    }
    result.ok_or(ReadError::Invalid)
}

fn constant(bytes: &[u8], end: usize, register: u16) -> ReadResult<(u32, RomSpan)> {
    let at = end.checked_sub(2).ok_or(ReadError::Invalid)?;
    let op = half(bytes, at).ok_or(ReadError::Invalid)?;
    if op & 0xff00 == 0x4800 | register << 8 {
        Ok((literal(bytes, at)?, literal_span(bytes, at)?))
    } else if op & 0xff00 == 0x2000 | register << 8 {
        Ok((u32::from(op & 255), span(bytes, at, 2)?))
    } else if op & 0xf83f == register << 3 | register {
        let before = at.checked_sub(2).ok_or(ReadError::Invalid)?;
        let mov = half(bytes, before).ok_or(ReadError::Invalid)?;
        if mov & 0xff00 != 0x2000 | register << 8 {
            return Err(ReadError::Invalid);
        }
        let shift = u32::from((op >> 6) & 31);
        Ok((
            u32::from(mov & 255)
                .checked_shl(shift)
                .ok_or(ReadError::Invalid)?,
            span(bytes, before, 4)?,
        ))
    } else {
        Err(ReadError::Invalid)
    }
}
