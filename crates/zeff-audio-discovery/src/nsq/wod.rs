use super::{Budget, ReadError, ReadResult, RomSpan, ScanStop, span, word};

const HEADER_BYTES: usize = 8;
const RECORD_BYTES: usize = 16;
const MAX_RECORDS: usize = 4096;
const MAX_FILESYSTEMS: usize = 16;
const MAX_PATH_BYTES: usize = 128;
const ROM_BASE: u32 = 0x0800_0000;

pub(super) struct Asset {
    pub span: RomSpan,
    pub flags: u32,
}

struct Filesystem {
    span: RomSpan,
    count: usize,
    multiplier: u32,
}

pub(super) fn find(
    bytes: &[u8],
    bank_name: &str,
    budget: &mut Budget<'_>,
) -> ReadResult<Vec<RomSpan>> {
    let mut filesystems = Vec::new();
    for offset in (0..bytes.len().saturating_sub(HEADER_BYTES - 1)).step_by(4) {
        budget.charge()?;
        let Some(filesystem) = candidate(bytes, offset, budget)? else {
            continue;
        };
        let Some(asset) = lookup(bytes, &filesystem, bank_name, budget)? else {
            continue;
        };
        if asset.flags != 0 || asset.span.byte_len != 128 * 48 {
            continue;
        }
        if filesystems.len() == MAX_FILESYSTEMS {
            return Err(ReadError::Stop(ScanStop::ValidationLimit));
        }
        filesystems
            .try_reserve(1)
            .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
        filesystems.push(filesystem.span);
    }
    Ok(filesystems)
}

#[allow(dead_code)]
#[cfg(test)]
pub(super) fn resolve(
    bytes: &[u8],
    filesystem: RomSpan,
    name: &str,
    budget: &mut Budget<'_>,
) -> ReadResult<RomSpan> {
    Ok(resolve_asset(bytes, filesystem, name, budget)?.span)
}

pub(super) fn resolve_asset(
    bytes: &[u8],
    filesystem: RomSpan,
    name: &str,
    budget: &mut Budget<'_>,
) -> ReadResult<Asset> {
    let parsed = parse(bytes, filesystem, budget)?;
    lookup(bytes, &parsed, name, budget)?.ok_or(ReadError::Invalid)
}

pub(super) fn path(bytes: &[u8], offset: usize) -> ReadResult<(String, RomSpan)> {
    let end = offset
        .checked_add(MAX_PATH_BYTES)
        .map_or(bytes.len(), |end| end.min(bytes.len()));
    let source = bytes.get(offset..end).ok_or(ReadError::Invalid)?;
    let Some(length) = source.iter().position(|&byte| byte == 0) else {
        return Err(ReadError::Invalid);
    };
    if length == 0
        || source[..length]
            .iter()
            .any(|&byte| !(0x20..=0x7e).contains(&byte) || byte == b'%')
    {
        return Err(ReadError::Invalid);
    }
    let name = String::from_utf8(source[..length].to_vec()).map_err(|_| ReadError::Invalid)?;
    Ok((name, span(bytes, offset, length + 1)?))
}

pub(super) fn pointer_offset(bytes: &[u8], value: u32) -> ReadResult<usize> {
    let offset = value.checked_sub(ROM_BASE).ok_or(ReadError::Invalid)? as usize;
    bytes.get(offset).ok_or(ReadError::Invalid)?;
    Ok(offset)
}

fn candidate(
    bytes: &[u8],
    offset: usize,
    budget: &mut Budget<'_>,
) -> ReadResult<Option<Filesystem>> {
    let Some(count) = word(bytes, offset).map(|value| value as usize) else {
        return Ok(None);
    };
    let Some(multiplier) = word(bytes, offset + 4) else {
        return Ok(None);
    };
    if !(1..=MAX_RECORDS).contains(&count) || !(2..=64).contains(&multiplier) {
        return Ok(None);
    }
    let Some(length) = count
        .checked_mul(RECORD_BYTES)
        .and_then(|length| length.checked_add(HEADER_BYTES))
    else {
        return Ok(None);
    };
    let Ok(table) = span(bytes, offset, length) else {
        return Ok(None);
    };
    let filesystem = Filesystem {
        span: table,
        count,
        multiplier,
    };
    if records_valid(bytes, &filesystem, budget)? {
        Ok(Some(filesystem))
    } else {
        Ok(None)
    }
}

fn parse(bytes: &[u8], span_value: RomSpan, budget: &mut Budget<'_>) -> ReadResult<Filesystem> {
    let offset = span_value.effective_offset as usize;
    let Some(count) = word(bytes, offset).map(|value| value as usize) else {
        return Err(ReadError::Invalid);
    };
    let multiplier = word(bytes, offset + 4).ok_or(ReadError::Invalid)?;
    if !(1..=MAX_RECORDS).contains(&count) || !(2..=64).contains(&multiplier) {
        return Err(ReadError::Invalid);
    }
    let length = count
        .checked_mul(RECORD_BYTES)
        .and_then(|value| value.checked_add(HEADER_BYTES))
        .ok_or(ReadError::Invalid)?;
    if span_value != span(bytes, offset, length)? {
        return Err(ReadError::Invalid);
    }
    let filesystem = Filesystem {
        span: span_value,
        count,
        multiplier,
    };
    if !records_valid(bytes, &filesystem, budget)? {
        return Err(ReadError::Invalid);
    }
    Ok(filesystem)
}

fn records_valid(
    bytes: &[u8],
    filesystem: &Filesystem,
    budget: &mut Budget<'_>,
) -> ReadResult<bool> {
    let base = filesystem.span.effective_offset as usize;
    let mut previous_data = 0;
    for index in 0..filesystem.count {
        budget.charge()?;
        let record = base + HEADER_BYTES + index * RECORD_BYTES;
        let length = word(bytes, record + 8).ok_or(ReadError::Invalid)? as usize;
        let relative = word(bytes, record + 12).ok_or(ReadError::Invalid)? as usize;
        let Some(data) = base.checked_add(relative) else {
            return Ok(false);
        };
        if (index != 0 && relative < previous_data)
            || length == 0
            || data.checked_add(length).is_none_or(|end| end > bytes.len())
        {
            return Ok(false);
        }
        previous_data = relative;
    }
    Ok(true)
}

fn lookup(
    bytes: &[u8],
    filesystem: &Filesystem,
    name: &str,
    budget: &mut Budget<'_>,
) -> ReadResult<Option<Asset>> {
    let expected = hash(name, filesystem.multiplier).ok_or(ReadError::Invalid)?;
    let base = filesystem.span.effective_offset as usize;
    let mut found = None;
    for index in 0..filesystem.count {
        budget.charge()?;
        let record = base + HEADER_BYTES + index * RECORD_BYTES;
        if word(bytes, record) != Some(expected) {
            continue;
        }
        let flags = word(bytes, record + 4).ok_or(ReadError::Invalid)?;
        let length = word(bytes, record + 8).ok_or(ReadError::Invalid)? as usize;
        let relative = word(bytes, record + 12).ok_or(ReadError::Invalid)? as usize;
        let offset = base.checked_add(relative).ok_or(ReadError::Invalid)?;
        let asset = Asset {
            span: span(bytes, offset, length)?,
            flags,
        };
        if found.replace(asset).is_some() {
            return Err(ReadError::Invalid);
        }
    }
    Ok(found)
}

fn hash(name: &str, multiplier: u32) -> Option<u32> {
    let mut value = 0u32;
    for byte in name.bytes() {
        if !byte.is_ascii() || byte == b'%' {
            return None;
        }
        value = value
            .wrapping_mul(multiplier)
            .wrapping_add(byte.to_ascii_lowercase() as u32);
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    const MULTIPLIER: u32 = 11;

    fn put(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn budget(cancel: &AtomicBool) -> Budget<'_> {
        Budget {
            cancel,
            remaining: 100_000,
        }
    }

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 0x2000];
        put(&mut bytes, 0, 2);
        put(&mut bytes, 4, MULTIPLIER);
        let bank = "audio\\instruments.npf";
        let raw = "audio\\patches\\midi0.raw";
        put(&mut bytes, 8, hash(bank, MULTIPLIER).unwrap());
        put(&mut bytes, 12, 0);
        put(&mut bytes, 16, 6144);
        put(&mut bytes, 20, 0x100);
        put(&mut bytes, 24, hash(raw, MULTIPLIER).unwrap());
        put(&mut bytes, 28, 0);
        put(&mut bytes, 32, 4);
        put(&mut bytes, 36, 0x1900);
        bytes
    }

    #[test]
    fn finds_and_resolves_a_bounded_unsorted_filesystem() {
        let bytes = fixture();
        let cancel = AtomicBool::new(false);
        let filesystems = find(&bytes, "audio\\instruments.npf", &mut budget(&cancel)).unwrap();
        assert_eq!(filesystems, [RomSpan::new(0, 40)]);
        assert_eq!(
            resolve(
                &bytes,
                filesystems[0],
                "audio\\patches\\midi0.raw",
                &mut budget(&cancel)
            )
            .unwrap(),
            RomSpan::new(0x1900, 4)
        );
    }

    #[test]
    fn duplicate_hashes_and_format_paths_are_rejected() {
        let mut bytes = fixture();
        put(
            &mut bytes,
            24,
            hash("audio\\instruments.npf", MULTIPLIER).unwrap(),
        );
        let cancel = AtomicBool::new(false);
        assert!(find(&bytes, "audio\\instruments.npf", &mut budget(&cancel)).is_err());
        bytes[0x80..0x85].copy_from_slice(b"bad%\0");
        assert!(path(&bytes, 0x80).is_err());
    }
}
