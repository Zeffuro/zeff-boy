use super::{Budget, EmbeddedFormat, EmbeddedModule, FileSpan, ModuleSource, ScanStop};

const MAX_CELLS: usize = 2 * 1024 * 1024;

pub(crate) fn parse(
    bytes: &[u8],
    base: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<EmbeddedModule>, ScanStop> {
    let mut stopped = None;
    let result = (|| {
        let data = bytes.get(base..)?;
        data.get(..96)?;
        if data.get(28) != Some(&0x1a)
            || data.get(29) != Some(&0x10)
            || data.get(44..48)? != b"SCRM"
            || !matches!(le16(data, 42)?, 1 | 2)
        {
            return None;
        }
        let orders = usize::from(le16(data, 32)?);
        let instruments = usize::from(le16(data, 34)?);
        let patterns = usize::from(le16(data, 36)?);
        if !(1..=256).contains(&orders)
            || instruments > 255
            || !(1..=255).contains(&patterns)
            || *data.get(48)? > 64
            || *data.get(49)? == 0
            || *data.get(50)? < 32
        {
            return None;
        }
        let table_end = 96usize
            .checked_add(orders)?
            .checked_add(instruments.checked_mul(2)?)?
            .checked_add(patterns.checked_mul(2)?)?;
        data.get(..table_end)?;
        let header_end = table_end.checked_add(if data[53] == 0xfc { 32 } else { 0 })?;
        data.get(..header_end)?;
        let order_data = data.get(96..96 + orders)?;
        if !order_data
            .iter()
            .any(|&order| order < 0xfe && usize::from(order) < patterns)
            || order_data
                .iter()
                .any(|&order| order < 0xfe && usize::from(order) >= patterns)
        {
            return None;
        }
        let channels = data[64..96].iter().rposition(|&channel| channel != 0xff)? + 1;
        let mut ordered = vec![false; patterns];
        for &pattern in order_data.iter().take_while(|&&pattern| pattern != 0xff) {
            if pattern != 0xfe {
                ordered[usize::from(pattern)] = true;
            }
        }

        let mut end = header_end;
        let mut occupied = vec![(0, header_end)];
        let mut referenced = [false; 256];
        let mut note_count = 0usize;
        let mut total_cells = 0usize;
        let pattern_table = 96 + orders + instruments * 2;
        for (index, &is_ordered) in ordered.iter().enumerate() {
            charge(budget, &mut stopped)?;
            let paragraph = usize::from(le16(data, pattern_table + index * 2)?);
            if paragraph == 0 {
                continue;
            }
            let start = paragraph.checked_mul(16)?;
            if start < table_end {
                return None;
            }
            let packed_len = usize::from(le16(data, start)?);
            if packed_len < 2 {
                return None;
            }
            // S3M counts the length word itself; IT's packed length does not.
            let packed_end = start.checked_add(packed_len)?;
            let packed = data.get(start + 2..packed_end)?;
            reserve(&mut occupied, start, packed_end)?;
            end = end.max(packed_end);
            let mut cursor = 0usize;
            let mut rows = 0usize;
            let mut current_instruments = [0usize; 32];
            while rows < 64 {
                charge(budget, &mut stopped)?;
                let control = *packed.get(cursor)?;
                cursor += 1;
                if control == 0 {
                    rows += 1;
                    continue;
                }
                total_cells += 1;
                if total_cells > MAX_CELLS {
                    return None;
                }
                if control & 0x20 != 0 {
                    let note = *packed.get(cursor)?;
                    let instrument = usize::from(*packed.get(cursor + 1)?);
                    cursor += 2;
                    if !valid_note(note) || instrument > instruments {
                        return None;
                    }
                    note_count += usize::from(note < 0xfe && is_ordered);
                    let channel = usize::from(control & 31);
                    if instrument != 0 {
                        current_instruments[channel] = instrument;
                    }
                    if note < 0xfe && is_ordered && data[64 + channel] != 0xff {
                        referenced[current_instruments[channel]] = true;
                    }
                }
                if control & 0x40 != 0 {
                    if *packed.get(cursor)? > 64 {
                        return None;
                    }
                    cursor += 1;
                }
                if control & 0x80 != 0 {
                    if *packed.get(cursor)? > 26 {
                        return None;
                    }
                    cursor += 2;
                }
            }
            if cursor != packed.len() {
                return None;
            }
        }

        let instrument_table = 96 + orders;
        let mut samples = 0u16;
        let mut sample_points = 0u32;
        let mut playable = false;
        for (index, &is_referenced) in referenced.iter().enumerate().take(instruments + 1).skip(1) {
            charge(budget, &mut stopped)?;
            let paragraph = usize::from(le16(data, instrument_table + (index - 1) * 2)?);
            let start = paragraph.checked_mul(16)?;
            if start < table_end {
                return None;
            }
            let header = data.get(start..start.checked_add(80)?)?;
            reserve(&mut occupied, start, start + 80)?;
            end = end.max(start + 80);
            match header[0] {
                0 => continue,
                1 => {}
                _ => return None,
            }
            if header.get(76..80)? != b"SCRS"
                || header[28] > 64
                || header[30] != 0
                || header[31] & !7 != 0
            {
                return None;
            }
            let points = le32(header, 16)? as usize;
            let loop_start = le32(header, 20)? as usize;
            let loop_end = le32(header, 24)? as usize;
            if header[31] & 1 != 0 && !(loop_start < loop_end && loop_end <= points) {
                return None;
            }
            let width = (1 + usize::from(header[31] & 4 != 0))
                .checked_mul(1 + usize::from(header[31] & 2 != 0))?;
            let bytes_len = points.checked_mul(width)?;
            if points != 0 {
                let paragraph = usize::from(le16(header, 14)?) | (usize::from(header[13]) << 16);
                let sample_start = paragraph.checked_mul(16)?;
                if sample_start < table_end {
                    return None;
                }
                let sample_end = sample_start.checked_add(bytes_len)?;
                data.get(sample_start..sample_end)?;
                reserve(&mut occupied, sample_start, sample_end)?;
                end = end.max(sample_end);
                samples = samples.checked_add(1)?;
                sample_points = sample_points.checked_add(points as u32)?;
                playable |= is_referenced;
            }
        }
        if note_count == 0 || !playable || end > super::super::MAX_ROM_BYTES {
            return None;
        }
        Some(EmbeddedModule {
            format: EmbeddedFormat::S3m,
            span: FileSpan {
                offset: base as u32,
                byte_len: end as u32,
            },
            name: super::inspect::text(data.get(..28)?),
            channels: channels as u16,
            orders: orders as u16,
            patterns: patterns as u16,
            instruments: instruments as u16,
            samples,
            sample_points,
            source: ModuleSource::Embedded,
        })
    })();
    match stopped {
        Some(reason) => Err(reason),
        None => Ok(result),
    }
}

fn valid_note(note: u8) -> bool {
    note >= 0xfe || (note >> 4 <= 8 && note & 0x0f <= 11)
}

fn reserve(occupied: &mut Vec<(usize, usize)>, start: usize, end: usize) -> Option<()> {
    if start >= end
        || occupied
            .iter()
            .any(|&(used_start, used_end)| start < used_end && used_start < end)
    {
        None
    } else {
        occupied.push((start, end));
        Some(())
    }
}

fn charge(budget: &mut Budget<'_>, stopped: &mut Option<ScanStop>) -> Option<()> {
    if let Err(reason) = budget.charge() {
        *stopped = Some(reason);
        None
    } else {
        Some(())
    }
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
