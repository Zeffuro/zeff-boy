use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::{
    Budget, MAX_CANDIDATES, MAX_ROM_BYTES, MAX_SCAN_WORK, ScanLimits, ScanStop, tracker::FileSpan,
};

const MARKER: &[u8; 8] = b"*maxmod*";
const PREFIX_LEN: usize = 8;
const HEADER_LEN: usize = 12;
const GUARD_LEN: usize = 4;
const MAX_PAYLOAD_LEN: usize = 1024 * 1024;
const MAX_SAMPLES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SampleBank {
    pub bank: FileSpan,
    pub table: FileSpan,
    pub samples: Vec<Sample>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Sample {
    pub record: FileSpan,
    pub header: FileSpan,
    pub payload: FileSpan,
    pub guard: FileSpan,
    pub frequency_code: u16,
}

pub fn discover(
    source: &[u8],
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> Result<Vec<SampleBank>, ScanStop> {
    preflight(source, limits, cancel)?;
    let mut budget = Budget {
        cancel,
        remaining: limits.max_work,
    };
    let mut banks = Vec::new();
    for offset in (0..source.len()).step_by(4) {
        budget.charge()?;
        let Some(bank) = inspect(source, offset, &mut budget)? else {
            continue;
        };
        if banks.len() >= limits.max_candidates as usize {
            return Err(ScanStop::CandidateLimit);
        }
        banks.push(bank);
    }
    Ok(banks)
}

fn preflight(source: &[u8], limits: ScanLimits, cancel: &AtomicBool) -> Result<(), ScanStop> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    if source.len() > MAX_ROM_BYTES {
        return Err(ScanStop::MediaLimit);
    }
    if limits.max_work > MAX_SCAN_WORK || limits.max_candidates > MAX_CANDIDATES {
        return Err(ScanStop::InvalidLimits);
    }
    Ok(())
}

fn inspect(
    source: &[u8],
    bank_start: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<SampleBank>, ScanStop> {
    let Some(marker_start) = bank_start.checked_add(4) else {
        return Ok(None);
    };
    let Some(marker_end) = bank_start.checked_add(12) else {
        return Ok(None);
    };
    let Some(marker) = source.get(marker_start..marker_end) else {
        return Ok(None);
    };
    if marker != MARKER {
        return Ok(None);
    }
    budget.charge()?;
    let Some(header) = source.get(bank_start..marker_end) else {
        return Ok(None);
    };
    let count = u16::from_le_bytes(header[..2].try_into().unwrap()) as usize;
    if !(1..=MAX_SAMPLES).contains(&count) || header[2..4] != [0, 0] {
        return Ok(None);
    }
    let Some(table_end) = marker_end.checked_add(count * 4) else {
        return Ok(None);
    };
    let Some(table) = source.get(marker_end..table_end) else {
        return Ok(None);
    };
    let mut samples = Vec::with_capacity(count);
    let mut end = table_end;
    for pointer in table.as_chunks::<4>().0 {
        budget.charge()?;
        let Some(record_start) = bank_start.checked_add(u32::from_le_bytes(*pointer) as usize)
        else {
            return Ok(None);
        };
        let Some(aligned) = end.checked_add(3).map(|value| value & !3) else {
            return Ok(None);
        };
        if record_start != aligned
            || !source
                .get(end..aligned)
                .is_some_and(|padding| padding.iter().all(|&byte| byte == 0xba))
        {
            return Ok(None);
        }
        let Some(sample) = inspect_sample(source, record_start) else {
            return Ok(None);
        };
        end = sample.guard.offset as usize + GUARD_LEN;
        samples.push(sample);
    }
    Ok(Some(SampleBank {
        bank: span(bank_start, end - bank_start),
        table: span(marker_end, count * 4),
        samples,
    }))
}

fn inspect_sample(source: &[u8], record_start: usize) -> Option<Sample> {
    let prefix_end = record_start.checked_add(PREFIX_LEN)?;
    let prefix = source.get(record_start..prefix_end)?;
    if prefix[4..] != [1, 0x18, 1, 0xBA] {
        return None;
    }
    let body_len = u32::from_le_bytes(prefix[..4].try_into().unwrap()) as usize;
    let body_start = prefix_end;
    let header_end = body_start.checked_add(HEADER_LEN)?;
    let header = source.get(body_start..header_end)?;
    let payload_len = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    if payload_len == 0 || payload_len > MAX_PAYLOAD_LEN {
        return None;
    }
    if u32::from_le_bytes(header[4..8].try_into().unwrap()) != u32::MAX
        || header[8] != 0
        || header[9] != 0xBA
        || body_len != payload_len + HEADER_LEN + GUARD_LEN
    {
        return None;
    }
    let payload_end = header_end.checked_add(payload_len)?;
    let guard_end = payload_end.checked_add(GUARD_LEN)?;
    if source.get(payload_end..guard_end) != Some(&[0x80; GUARD_LEN]) {
        return None;
    }
    Some(Sample {
        record: span(record_start, PREFIX_LEN + body_len),
        header: span(body_start, HEADER_LEN),
        payload: span(header_end, payload_len),
        guard: span(payload_end, GUARD_LEN),
        frequency_code: u16::from_le_bytes(header[10..12].try_into().unwrap()),
    })
}

fn span(offset: usize, byte_len: usize) -> FileSpan {
    FileSpan {
        offset: offset as u32,
        byte_len: byte_len as u32,
    }
}

#[cfg(test)]
mod tests;
