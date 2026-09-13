use anyhow::{Context, ensure};

use super::{GaxNativeSong, MAX_ROM_BYTES, word};

const TAIL_BYTES: usize = 65536;
const FOOTER_BYTES: usize = 256;
const DRIVER_GUARD_BYTES: usize = 65536;

pub(super) fn choose(bytes: &[u8], song: &GaxNativeSong, length: usize) -> Option<usize> {
    if !(0xc0..=MAX_ROM_BYTES).contains(&bytes.len()) || length == 0 || length > TAIL_BYTES {
        return None;
    }
    let aligned = bytes.len().checked_add(3)? & !3;
    if aligned.checked_add(length)? <= MAX_ROM_BYTES {
        return Some(aligned);
    }
    let reset = word(bytes, 0)?;
    if reset & 0xff00_0000 != 0xea00_0000 {
        return None;
    }
    let displacement = ((reset << 8) as i32 >> 6) as i64;
    let entry = usize::try_from(8 + displacement).ok()?;
    if entry >= bytes.len() {
        return None;
    }
    let end = (bytes.len().checked_sub(FOOTER_BYTES)?) & !3;
    let lower = bytes.len().saturating_sub(TAIL_BYTES).max(0xc0);
    let mut protected = Vec::new();
    protected.push(entry..entry.saturating_add(4096));
    let native = &song.native;
    let entries = native
        .new
        .iter()
        .chain([&native.init, &native.mix, &native.play]);
    for entry in entries {
        let start = entry.source.effective_offset as usize;
        protected.push(start.saturating_sub(256)..start.saturating_add(DRIVER_GUARD_BYTES));
    }
    for span in song
        .mapped_spans
        .iter()
        .chain([&song.header])
        .chain(native.ram_copies.iter().map(|copy| &copy.source))
    {
        let start = span.effective_offset as usize;
        let span_end = start.checked_add(span.byte_len as usize)?;
        if span_end > bytes.len() {
            return None;
        }
        if span_end > lower && start < end {
            protected.push(start..span_end);
        }
    }
    let mut cursor = end;
    while cursor > lower {
        let value = bytes[cursor - 1];
        if !matches!(value, 0 | 255) {
            cursor -= 1;
            continue;
        }
        let run_end = cursor;
        while cursor > lower && bytes[cursor - 1] == value {
            cursor -= 1;
        }
        let Some(mut offset) = run_end.checked_sub(length).map(|at| at & !3) else {
            continue;
        };
        while offset >= cursor {
            let overlapping = protected
                .iter()
                .filter(|span| offset < span.end && offset + length > span.start)
                .map(|span| span.start)
                .min();
            let Some(start) = overlapping else {
                return Some(offset);
            };
            let Some(previous) = start.checked_sub(length) else {
                break;
            };
            offset = previous & !3;
        }
    }
    None
}

pub(super) fn install(bytes: &[u8], offset: usize, payload: &[u8]) -> anyhow::Result<Vec<u8>> {
    let end = offset
        .checked_add(payload.len())
        .context("GAX bootstrap length overflow")?;
    ensure!(
        bytes.len() >= 4
            && bytes.len() <= MAX_ROM_BYTES
            && offset >= 0xc0
            && offset.is_multiple_of(4)
            && end <= MAX_ROM_BYTES,
        "GAX bootstrap placement is out of range"
    );
    let mut result = bytes.to_vec();
    result.resize(bytes.len().max(end), 0);
    result[offset..end].copy_from_slice(payload);
    let branch = 0xea00_0000 | (((offset as u32 - 8) >> 2) & 0x00ff_ffff);
    result[..4].copy_from_slice(&branch.to_le_bytes());
    Ok(result)
}

#[cfg(test)]
mod tests;
