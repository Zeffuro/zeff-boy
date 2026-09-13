use super::{Budget, EmbeddedFormat, EmbeddedModule, FileSpan, ModuleSource, ScanStop};

const HEADER_LEN: usize = 192;
const INSTRUMENT_LEN: usize = 554;
const SAMPLE_LEN: usize = 80;
const MAX_CELLS: usize = 2 * 1024 * 1024;

pub(crate) fn parse(
    bytes: &[u8],
    base: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<EmbeddedModule>, ScanStop> {
    let mut stopped = None;
    let result = (|| {
        let data = bytes.get(base..)?;
        data.get(..HEADER_LEN)?;
        if data.get(..4)? != b"IMPM" {
            return None;
        }
        let orders = usize::from(le16(data, 32)?);
        let instruments = usize::from(le16(data, 34)?);
        let sample_count = usize::from(le16(data, 36)?);
        let patterns = usize::from(le16(data, 38)?);
        let compatible_with = le16(data, 42)?;
        let flags = le16(data, 44)?;
        let special = le16(data, 46)?;
        if !(1..=256).contains(&orders)
            || instruments > 255
            || !(1..=255).contains(&sample_count)
            || !(1..=255).contains(&patterns)
            || !(0x0100..=0x0217).contains(&compatible_with)
            || (flags & 4 != 0 && compatible_with < 0x0200)
            || special & !15 != 0
            || data[48] > 128
            || data[50] == 0
            || data[51] < 32
            || data[128..192].iter().any(|&volume| volume > 64)
        {
            return None;
        }
        if flags & 4 == 0 && instruments != 0 {
            return None;
        }
        if data[64..128]
            .iter()
            .any(|&pan| (pan & 0x7f) > 64 && (pan & 0x7f) != 100)
        {
            return None;
        }
        let table_end = HEADER_LEN
            .checked_add(orders)?
            .checked_add(instruments.checked_mul(4)?)?
            .checked_add(sample_count.checked_mul(4)?)?
            .checked_add(patterns.checked_mul(4)?)?;
        data.get(..table_end)?;
        let order_data = data.get(HEADER_LEN..HEADER_LEN + orders)?;
        if !order_data
            .iter()
            .any(|&order| order < 0xfe && usize::from(order) < patterns)
            || order_data
                .iter()
                .any(|&order| order < 0xfe && usize::from(order) >= patterns)
        {
            return None;
        }

        let instrument_table = HEADER_LEN + orders;
        let sample_table = instrument_table + instruments * 4;
        let pattern_table = sample_table + sample_count * 4;
        let mut ordered = vec![false; patterns];
        for &pattern in order_data.iter().take_while(|&&pattern| pattern != 0xff) {
            if pattern != 0xfe {
                ordered[usize::from(pattern)] = true;
            }
        }
        let mut header_end = table_end;
        if special & 2 != 0 {
            let entries = usize::from(le16(data, header_end)?);
            header_end = header_end
                .checked_add(2)?
                .checked_add(entries.checked_mul(8)?)?;
        }
        if flags & 0x80 != 0 || special & 8 != 0 {
            header_end = header_end.checked_add((9 + 16 + 128) * 32)?;
        }
        data.get(..header_end)?;
        let mut end = header_end;
        let mut occupied = vec![(0, header_end)];
        if special & 1 != 0 {
            let length = usize::from(le16(data, 54)?);
            if length != 0 {
                let start = le32(data, 56)? as usize;
                let message_end = start.checked_add(length)?;
                data.get(start..message_end)?;
                reserve(&mut occupied, start, message_end)?;
                end = end.max(message_end);
            }
        }
        let mut referenced_keys = vec![[false; 120]; 256];
        let mut note_count = 0usize;
        let mut total_cells = 0usize;
        let mut channels = 0u16;
        for (index, &is_ordered) in ordered.iter().enumerate() {
            charge(budget, &mut stopped)?;
            let start = le32(data, pattern_table + index * 4)? as usize;
            if start == 0 {
                total_cells = total_cells.checked_add(64 * 64)?;
                if total_cells > MAX_CELLS {
                    return None;
                }
                continue;
            }
            if start < table_end {
                return None;
            }
            let packed_len = usize::from(le16(data, start)?);
            let rows = usize::from(le16(data, start + 2)?);
            if !(1..=1024).contains(&rows) {
                return None;
            }
            total_cells = total_cells.checked_add(rows.checked_mul(64)?)?;
            if total_cells > MAX_CELLS {
                return None;
            }
            let packed_end = start.checked_add(8)?.checked_add(packed_len)?;
            let packed = data.get(start + 8..packed_end)?;
            reserve(&mut occupied, start, packed_end)?;
            end = end.max(packed_end);
            let mut masks = [0u8; 64];
            let mut previous_notes = [None; 64];
            let mut previous_instruments = [0u8; 64];
            let mut current_instruments = [0u8; 64];
            let mut cursor = 0usize;
            let mut row = 0usize;
            while row < rows {
                charge(budget, &mut stopped)?;
                let control = *packed.get(cursor)?;
                cursor += 1;
                if control == 0 {
                    row += 1;
                    continue;
                }
                let channel = usize::from((control - 1) & 63);
                channels = channels.max(channel as u16 + 1);
                if control & 0x80 != 0 {
                    masks[channel] = *packed.get(cursor)?;
                    cursor += 1;
                }
                let mask = masks[channel];
                let mut played_note = None;
                if mask & 1 != 0 {
                    let note = *packed.get(cursor)?;
                    cursor += 1;
                    if note > 119 && note < 253 {
                        return None;
                    }
                    previous_notes[channel] = Some(note);
                    played_note = Some(note);
                } else if mask & 0x10 != 0 {
                    played_note = previous_notes[channel];
                }
                if mask & 2 != 0 {
                    let instrument = usize::from(*packed.get(cursor)?);
                    cursor += 1;
                    let maximum = if flags & 4 != 0 {
                        instruments
                    } else {
                        sample_count
                    };
                    if instrument > maximum {
                        return None;
                    }
                    previous_instruments[channel] = instrument as u8;
                }
                if mask & 0x22 != 0 && previous_instruments[channel] != 0 {
                    current_instruments[channel] = previous_instruments[channel];
                }
                if let Some(note) = played_note.filter(|&note| note <= 119 && is_ordered) {
                    note_count += 1;
                    referenced_keys[usize::from(current_instruments[channel])][usize::from(note)] =
                        true;
                }
                if mask & 4 != 0 {
                    let volume = *packed.get(cursor)?;
                    cursor += 1;
                    if volume > 124 && !(128..=212).contains(&volume) {
                        return None;
                    }
                }
                if mask & 8 != 0 {
                    if *packed.get(cursor)? > 26 {
                        return None;
                    }
                    cursor = cursor.checked_add(2)?;
                    packed.get(cursor - 2..cursor)?;
                }
            }
            if cursor != packed.len() {
                return None;
            }
        }

        let mut referenced_samples = [false; 257];
        if flags & 4 != 0 {
            for (index, keys) in referenced_keys
                .iter()
                .enumerate()
                .take(instruments + 1)
                .skip(1)
            {
                charge(budget, &mut stopped)?;
                let start = le32(data, instrument_table + (index - 1) * 4)? as usize;
                if start == 0 {
                    continue;
                }
                if start < table_end
                    || data
                        .get(start..start.checked_add(INSTRUMENT_LEN)?)?
                        .get(..4)?
                        != b"IMPI"
                {
                    return None;
                }
                reserve(&mut occupied, start, start + INSTRUMENT_LEN)?;
                end = end.max(start + INSTRUMENT_LEN);
                for (key, &used) in keys.iter().enumerate() {
                    let mapped_note = data[start + 64 + key * 2];
                    let sample = usize::from(data[start + 64 + key * 2 + 1]);
                    if sample > sample_count || mapped_note > 119 {
                        return None;
                    }
                    if used {
                        referenced_samples[sample] = true;
                    }
                }
            }
        } else {
            for index in 1..=sample_count {
                referenced_samples[index] = referenced_keys[index].iter().any(|&used| used);
            }
        }

        let mut samples = 0u16;
        let mut sample_points = 0u32;
        let mut playable = false;
        for (index, &is_referenced) in referenced_samples
            .iter()
            .enumerate()
            .take(sample_count + 1)
            .skip(1)
        {
            charge(budget, &mut stopped)?;
            let start = le32(data, sample_table + (index - 1) * 4)? as usize;
            if start == 0 {
                continue;
            }
            if start < table_end {
                return None;
            }
            let header = data.get(start..start.checked_add(SAMPLE_LEN)?)?;
            if header.get(..4)? != b"IMPS"
                || header[17] > 64
                || header[19] > 64
                || header[47] & 0x7f > 64
            {
                return None;
            }
            end = end.max(start + SAMPLE_LEN);
            reserve(&mut occupied, start, start + SAMPLE_LEN)?;
            let flags = header[18];
            let points = le32(header, 48)? as usize;
            let loop_start = le32(header, 52)? as usize;
            let loop_end = le32(header, 56)? as usize;
            let sustain_start = le32(header, 64)? as usize;
            let sustain_end = le32(header, 68)? as usize;
            if flags & 0x10 != 0 && !(loop_start < loop_end && loop_end <= points)
                || flags & 0x20 != 0 && !(sustain_start < sustain_end && sustain_end <= points)
                || flags & 0x08 != 0
                || header[46] == 0xff
                || header[46] & !0x1f != 0
                || flags & 1 == 0 && points != 0
            {
                return None;
            }
            if flags & 1 == 0 {
                continue;
            }
            if points == 0 {
                continue;
            }
            let width =
                (1 + usize::from(flags & 2 != 0)).checked_mul(1 + usize::from(flags & 4 != 0))?;
            let payload_len = points.checked_mul(width)?;
            let payload_start = le32(header, 72)? as usize;
            if payload_start < table_end {
                return None;
            }
            let payload_end = payload_start.checked_add(payload_len)?;
            data.get(payload_start..payload_end)?;
            reserve(&mut occupied, payload_start, payload_end)?;
            end = end.max(payload_end);
            samples = samples.checked_add(1)?;
            sample_points = sample_points.checked_add(points as u32)?;
            playable |= is_referenced && points != 0;
        }
        if note_count == 0 || !playable || end > super::super::MAX_ROM_BYTES {
            return None;
        }
        Some(EmbeddedModule {
            format: EmbeddedFormat::It,
            span: FileSpan {
                offset: base as u32,
                byte_len: end as u32,
            },
            name: super::inspect::text(data.get(4..30)?),
            channels,
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

fn charge(budget: &mut Budget<'_>, stopped: &mut Option<ScanStop>) -> Option<()> {
    if let Err(reason) = budget.charge() {
        *stopped = Some(reason);
        None
    } else {
        Some(())
    }
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
