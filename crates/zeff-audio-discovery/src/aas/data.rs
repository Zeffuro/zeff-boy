use std::collections::BTreeSet;

use super::{
    AasNativeProfile, AasSong, Budget, INSTRUMENTS_PER_SONG, MAX_ORDERS, PATTERN_BYTES,
    PATTERN_ROWS, ReadError, ReadResult, SAMPLE_HEADER_BYTES, half, span, word,
};

pub(super) fn parse_song(
    bytes: &[u8],
    native: &AasNativeProfile,
    index: u16,
    budget: &mut Budget<'_>,
) -> ReadResult<AasSong> {
    budget.charge()?;
    let tables = &native.tables;
    let count = half(bytes, tables.count.effective_offset as usize).ok_or(ReadError::Invalid)?;
    if index >= count {
        return Err(ReadError::Invalid);
    }
    let selected = usize::from(index);
    let channels = *bytes
        .get(tables.channels.effective_offset as usize + selected)
        .ok_or(ReadError::Invalid)?;
    if !(1..=native.max_channels).contains(&channels) {
        return Err(ReadError::Invalid);
    }
    let stride = usize::from(native.max_channels) * 2;
    let header = span(
        bytes,
        tables.sequence.effective_offset as usize + selected * MAX_ORDERS * stride,
        MAX_ORDERS * stride,
    )?;
    let mut spans = vec![
        tables.count,
        header,
        span(
            bytes,
            tables.channels.effective_offset as usize + selected,
            1,
        )?,
        span(
            bytes,
            tables.restart.effective_offset as usize + selected,
            1,
        )?,
    ];
    let mut patterns = BTreeSet::new();
    let mut orders = 0;
    for order in 0..MAX_ORDERS {
        budget.charge()?;
        let at = header.effective_offset as usize + order * stride;
        if half(bytes, at) == Some(u16::MAX) {
            break;
        }
        for channel in 0..usize::from(channels) {
            budget.charge()?;
            let pattern = half(bytes, at + channel * 2).ok_or(ReadError::Invalid)? as i16;
            if pattern < 0 {
                return Err(ReadError::Invalid);
            }
            patterns.insert(pattern as usize);
        }
        orders += 1;
    }
    let restart = bytes[tables.restart.effective_offset as usize + selected];
    if orders == 0 || orders == MAX_ORDERS || usize::from(restart) >= orders {
        return Err(ReadError::Invalid);
    }
    let mut notes = 0;
    let mut used_instruments = BTreeSet::new();
    for &pattern in &patterns {
        let data = span(
            bytes,
            tables.pattern_data.effective_offset as usize + pattern * PATTERN_BYTES,
            PATTERN_BYTES,
        )?;
        spans.push(data);
        for row in 0..PATTERN_ROWS {
            budget.charge()?;
            let cell =
                word(bytes, data.effective_offset as usize + row * 4).ok_or(ReadError::Invalid)?;
            let instrument = cell >> 24;
            let note = (cell >> 12) & 0xfff;
            if instrument > INSTRUMENTS_PER_SONG as u32 || note > 60 {
                return Err(ReadError::Invalid);
            }
            notes += u32::from(note != 0);
            if instrument != 0 {
                used_instruments.insert(instrument);
            }
        }
    }
    let sample_headers = span(
        bytes,
        tables.sample_headers.effective_offset as usize
            + selected * INSTRUMENTS_PER_SONG * SAMPLE_HEADER_BYTES,
        INSTRUMENTS_PER_SONG * SAMPLE_HEADER_BYTES,
    )?;
    spans.push(sample_headers);
    let mut samples = 0;
    for sample in 0..INSTRUMENTS_PER_SONG {
        budget.charge()?;
        let at = sample_headers.effective_offset as usize + sample * SAMPLE_HEADER_BYTES;
        let offset = word(bytes, at).ok_or(ReadError::Invalid)? as usize;
        let repeat = half(bytes, at + 4).ok_or(ReadError::Invalid)?;
        let length = half(bytes, at + 6).ok_or(ReadError::Invalid)?;
        if (repeat != u16::MAX && repeat > length)
            || bytes[at + 8] > 15
            || bytes[at + 9] > 64
            || half(bytes, at + 10) != Some(0)
        {
            return Err(ReadError::Invalid);
        }
        if length != 0 {
            spans.push(span(
                bytes,
                (tables.sample_data.effective_offset as usize)
                    .checked_add(offset)
                    .ok_or(ReadError::Invalid)?,
                usize::from(length) * 2,
            )?);
            samples += 1;
        }
    }
    spans.extend(native.setup_spans.iter().copied());
    spans.sort_unstable();
    spans.dedup();
    Ok(AasSong {
        root: tables.count, header, index, title: format!("Module {index}"), channels,
        orders: orders as u16, patterns: patterns.len() as u16, notes,
        instruments: used_instruments.len() as u16, samples, native: native.clone(), mapped_spans: spans,
        warnings: vec!["Runs the original AAS MOD driver with the source's initialization settings in an isolated GBA emulator.".to_owned()],
    })
}
