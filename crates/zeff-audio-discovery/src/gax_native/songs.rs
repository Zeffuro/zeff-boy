use super::{Budget, ParseError, RomSpan, half, pointer, rom_pointer, word};

const MAX_HANDLERS: usize = 255;
const MAX_TITLE_BYTES: usize = 512;

#[derive(Clone, Copy)]
struct Handler {
    source: RomSpan,
    data: usize,
    linked: Option<(usize, usize)>,
}

pub(super) fn parse_song_layout(
    bytes: &[u8],
    offset: usize,
    budget: &mut Budget<'_>,
    header_bytes: usize,
    notes_offset: usize,
) -> Result<(u16, String, Vec<RomSpan>), ParseError> {
    let handler_count = word(bytes, offset).ok_or(ParseError::Invalid)? as usize;
    let minimum_handlers = if header_bytes == 20 { 3 } else { 4 };
    if !(minimum_handlers..=MAX_HANDLERS).contains(&handler_count) {
        return Err(ParseError::Invalid);
    }
    let table_len = handler_count.checked_mul(4).ok_or(ParseError::Invalid)?;
    let table_end = offset
        .checked_add(4 + table_len)
        .ok_or(ParseError::Invalid)?;
    bytes.get(offset..table_end).ok_or(ParseError::Invalid)?;
    for index in 0..handler_count {
        budget.charge()?;
        let address = word(bytes, offset + 4 + index * 4).ok_or(ParseError::Invalid)?;
        if address == 0 && index == 2 {
            continue;
        }
        rom_pointer(bytes, address, 1, 1).ok_or(ParseError::Invalid)?;
    }
    let patterns = parse_handler(
        bytes,
        rom_pointer(
            bytes,
            word(bytes, offset + 4).ok_or(ParseError::Invalid)?,
            28,
            4,
        )
        .ok_or(ParseError::Invalid)?,
        budget,
    )?;
    let header_handler = parse_handler(
        bytes,
        rom_pointer(
            bytes,
            word(bytes, offset + 8).ok_or(ParseError::Invalid)?,
            28,
            4,
        )
        .ok_or(ParseError::Invalid)?,
        budget,
    )?;
    let header = header_handler.data;
    let channels = half(bytes, header).ok_or(ParseError::Invalid)?;
    if !(1..=32).contains(&channels) {
        return Err(ParseError::Invalid);
    }
    let notes = pointer(bytes, header + notes_offset, 1, 4).ok_or(ParseError::Invalid)?;
    let instruments = pointer(bytes, header + notes_offset + 4, 4, 4).ok_or(ParseError::Invalid)?;
    let samples = pointer(bytes, header + notes_offset + 8, 8, 4).ok_or(ParseError::Invalid)?;
    let first_instrument = pointer(bytes, instruments, 1, 1).ok_or(ParseError::Invalid)?;
    let sample_pointer = word(bytes, samples).ok_or(ParseError::Invalid)?;
    if sample_pointer != 0
        && (rom_pointer(bytes, sample_pointer, 1, 1).is_none()
            || word(bytes, samples + 4) != Some(0))
    {
        return Err(ParseError::Invalid);
    }
    let linked = patterns.linked.ok_or(ParseError::Invalid)?;
    let mut min_pattern_data = usize::MAX;
    for index in 0..linked.1 {
        budget.charge()?;
        let address = word(bytes, linked.0 + index * 4).ok_or(ParseError::Invalid)?;
        let handler = parse_handler(
            bytes,
            rom_pointer(bytes, address, 28, 4).ok_or(ParseError::Invalid)?,
            budget,
        )?;
        min_pattern_data = min_pattern_data.min(handler.data);
    }
    if min_pattern_data == usize::MAX {
        return Err(ParseError::Invalid);
    }
    let title = title_before(bytes, min_pattern_data, budget)?;
    let mut spans = vec![
        RomSpan::new(offset, 4 + table_len),
        RomSpan::new(header, header_bytes),
        RomSpan::new(notes, 1),
        RomSpan::new(instruments, 4),
        RomSpan::new(first_instrument, 1),
        RomSpan::new(samples, 8),
        patterns.source,
        header_handler.source,
        RomSpan::new(linked.0, linked.1 * 4),
    ];
    if sample_pointer != 0 {
        spans.push(RomSpan::new(
            rom_pointer(bytes, sample_pointer, 1, 1).ok_or(ParseError::Invalid)?,
            1,
        ));
    }
    Ok((channels, title, spans))
}

fn parse_handler(
    bytes: &[u8],
    offset: usize,
    budget: &mut Budget<'_>,
) -> Result<Handler, ParseError> {
    budget.charge()?;
    let init = word(bytes, offset).ok_or(ParseError::Invalid)?;
    let unknown = word(bytes, offset + 4).ok_or(ParseError::Invalid)?;
    let play = word(bytes, offset + 8).ok_or(ParseError::Invalid)?;
    if [init, unknown, play]
        .into_iter()
        .any(|address| rom_pointer(bytes, address, 1, 1).is_none())
    {
        return Err(ParseError::Invalid);
    }
    let count = word(bytes, offset + 12).ok_or(ParseError::Invalid)? as usize;
    if count > MAX_HANDLERS {
        return Err(ParseError::Invalid);
    }
    let linked = if count == 0 {
        None
    } else {
        let start = pointer(
            bytes,
            offset + 16,
            count.checked_mul(4).ok_or(ParseError::Invalid)?,
            4,
        )
        .ok_or(ParseError::Invalid)?;
        Some((start, count))
    };
    let data = pointer(bytes, offset + 24, 1, 1).ok_or(ParseError::Invalid)?;
    Ok(Handler {
        source: RomSpan::new(offset, 28),
        data,
        linked,
    })
}

fn title_before(
    bytes: &[u8],
    mut end: usize,
    budget: &mut Budget<'_>,
) -> Result<String, ParseError> {
    let floor = end.saturating_sub(MAX_TITLE_BYTES);
    while end > floor && bytes[end - 1] == 0 && !end.is_multiple_of(4) {
        budget.charge()?;
        end -= 1;
    }
    while end > floor && bytes[end - 1] == 0 {
        budget.charge()?;
        end -= 1;
    }
    let mut start = end;
    let mut quotes = 0;
    while start > floor {
        budget.charge()?;
        let byte = bytes[start - 1];
        let printable = (0x20..=0x7e).contains(&byte)
            || (0x80..=0xff).contains(&byte) && !matches!(byte, 0x81 | 0x8d | 0x8f | 0x90 | 0x9d);
        if !printable {
            break;
        }
        start -= 1;
        if byte == b'"' {
            quotes += 1;
            if quotes == 2 {
                break;
            }
        }
    }
    Ok(bytes[start..end]
        .iter()
        .map(|byte| char::from(*byte))
        .collect())
}
