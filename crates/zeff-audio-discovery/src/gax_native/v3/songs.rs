use std::collections::BTreeSet;

use super::super::{Budget, RomSpan, ScanStop, half, word};

pub(super) struct Song {
    pub channels: u16,
    pub title: String,
    pub spans: Vec<RomSpan>,
}

enum Error {
    Invalid,
    Stop(ScanStop),
}
impl From<ScanStop> for Error {
    fn from(stop: ScanStop) -> Self {
        Self::Stop(stop)
    }
}
type Result<T> = std::result::Result<T, Error>;

pub(super) fn read(
    bytes: &[u8],
    at: usize,
    budget: &mut Budget<'_>,
) -> std::result::Result<Option<Song>, ScanStop> {
    match read_song(bytes, at, budget) {
        Ok(song) => Ok(Some(song)),
        Err(Error::Invalid) => Ok(None),
        Err(Error::Stop(stop)) => Err(stop),
    }
}

fn read_song(bytes: &[u8], at: usize, budget: &mut Budget<'_>) -> Result<Song> {
    need(bytes.get(at..at + 200))?;
    let channels = need(half(bytes, at))?;
    let rows = usize::from(need(half(bytes, at + 2))?);
    let orders = usize::from(need(half(bytes, at + 4))?);
    check(
        (1..=32).contains(&channels) && (1..=511).contains(&rows) && (1..=255).contains(&orders),
    )?;
    check(usize::from(channels) * rows * orders <= 262_144)?;
    check(
        usize::from(need(half(bytes, at + 6))?) < orders
            && (1_000..=65_535).contains(&need(half(bytes, at + 24))?)
            && bytes[at + 28] <= 32,
    )?;
    let sequence = pointer(bytes, at + 12, 1, 1)?;
    let instruments = pointer(bytes, at + 16, 4, 4)?;
    let samples = pointer(bytes, at + 20, 8, 4)?;
    let mut spans = vec![
        RomSpan::new(at, 200),
        RomSpan::new(instruments, 4),
        RomSpan::new(samples, 8),
    ];
    let mut patterns = BTreeSet::new();
    let mut used = BTreeSet::new();
    let mut metadata_start = sequence;
    let mut metadata_end = bytes.len();
    let mut pitched_notes = 0;
    for channel in 0..32 {
        budget.charge()?;
        let slot = at + 32 + channel * 4;
        if channel >= usize::from(channels) {
            check(word(bytes, slot) == Some(0))?;
            continue;
        }
        let table = pointer(bytes, slot, orders * 4, 4)?;
        metadata_end = metadata_end.min(table);
        spans.push(RomSpan::new(table, orders * 4));
        for order in 0..orders {
            budget.charge()?;
            let entry = table + order * 4;
            check(bytes[entry + 3] == 0)?;
            let offset = sequence
                .checked_add(usize::from(need(half(bytes, entry))?))
                .ok_or(Error::Invalid)?;
            if !patterns.insert(offset) {
                continue;
            }
            if patterns.len() > 512 {
                return Err(Error::Stop(ScanStop::ValidationLimit));
            }
            let (end, notes) = pattern(bytes, offset, rows, &mut used, budget)?;
            pitched_notes += notes;
            metadata_start = metadata_start.max(end);
            spans.push(RomSpan::new(offset, end - offset));
        }
    }
    check(pitched_notes > 0 && !used.is_empty())?;
    let title = title(bytes, metadata_start, metadata_end)?;
    spans.push(RomSpan::new(metadata_start, metadata_end - metadata_start));
    let mut sample_indices = BTreeSet::new();
    for index in used {
        budget.charge()?;
        let slot = instruments + usize::from(index) * 4;
        let instrument = pointer(bytes, slot, 24, 4)?;
        check(bytes[instrument] <= 1)?;
        spans.extend([RomSpan::new(slot, 4), RomSpan::new(instrument, 24)]);
        sample_indices.extend(
            bytes[instrument + 1..instrument + 5]
                .iter()
                .copied()
                .filter(|&index| index != 0),
        );
        if bytes[instrument + 17] != 0
            && word(bytes, instrument + 20)
                .is_some_and(|value| (0x0800_0000..0x0a00_0000).contains(&value))
        {
            let length = usize::from(bytes[instrument + 17]) * 8;
            let rows = pointer(bytes, instrument + 20, length, 4)?;
            spans.push(RomSpan::new(rows, length));
        }
    }
    let mut total = 0usize;
    for index in sample_indices {
        budget.charge()?;
        let slot = samples + usize::from(index) * 8;
        let length = need(word(bytes, slot + 4))? as usize;
        check(length > 0)?;
        total = total.checked_add(length).ok_or(Error::Invalid)?;
        check(total <= super::super::MAX_ROM_BYTES)?;
        let data = pointer(bytes, slot, length, 1)?;
        spans.extend([RomSpan::new(slot, 8), RomSpan::new(data, length)]);
    }
    check(total > 0)?;
    Ok(Song {
        channels,
        title,
        spans,
    })
}

fn pattern(
    bytes: &[u8],
    at: usize,
    rows: usize,
    used: &mut BTreeSet<u8>,
    budget: &mut Budget<'_>,
) -> Result<(usize, usize)> {
    let empty = *need(bytes.get(at))?;
    check(empty <= 1)?;
    let mut cursor = at + 1;
    let mut row = if empty == 1 { rows } else { 0 };
    let mut notes = 0;
    while row < rows {
        budget.charge()?;
        let flag = *need(bytes.get(cursor))?;
        cursor += 1;
        match flag {
            0x80 => (),
            0xff => {
                let count = usize::from(*need(bytes.get(cursor))?);
                cursor += 1;
                check(count > 0 && row + count <= rows)?;
                row += count;
                continue;
            }
            0xfa..=0xfe => {
                need(bytes.get(cursor..cursor + 2))?;
                cursor += 2;
            }
            _ => {
                let instrument = *need(bytes.get(cursor))?;
                cursor += 1;
                if instrument != 0 {
                    used.insert(instrument);
                }
                if flag & 0x7f > 1 {
                    notes += 1;
                }
                if flag & 0x80 == 0 {
                    need(bytes.get(cursor..cursor + 2))?;
                    cursor += 2;
                }
            }
        }
        row += 1;
    }
    Ok((cursor, notes))
}

fn title(bytes: &[u8], start: usize, end: usize) -> Result<String> {
    check(end > start && end - start <= 256)?;
    let raw = need(bytes.get(start..end))?;
    let padding = raw.iter().rev().take_while(|&&value| value == 0).count();
    check(padding <= 3)?;
    let raw = &raw[..raw.len() - padding];
    check(raw.first() == Some(&b'"'))?;
    let close = need(
        raw.iter()
            .enumerate()
            .skip(1)
            .find(|(_, value)| **value == b'"')
            .map(|(i, _)| i),
    )?;
    check(
        raw.get(close + 1..close + 4) == Some(&[b' ', 0xa9, b' '])
            && raw.len() > close + 4
            && raw.iter().all(|&value| value >= 32 && value != 127),
    )?;
    Ok(raw[1..close].iter().copied().map(char::from).collect())
}

fn pointer(bytes: &[u8], at: usize, length: usize, align: usize) -> Result<usize> {
    need(super::super::super::rom_pointer(
        bytes,
        need(word(bytes, at))?,
        length,
        align,
    ))
}
fn need<T>(value: Option<T>) -> Result<T> {
    value.ok_or(Error::Invalid)
}
fn check(value: bool) -> Result<()> {
    value.then_some(()).ok_or(Error::Invalid)
}
