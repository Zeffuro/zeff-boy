use super::{Budget, EmbeddedFormat, EmbeddedModule, FileSpan, ModuleSource, ScanStop};

const MAX_CELLS: usize = 2 * 1024 * 1024;

pub(crate) fn xm(
    bytes: &[u8],
    base: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<EmbeddedModule>, ScanStop> {
    let mut stopped = None;
    let result = (|| {
        let data = bytes.get(base..)?;
        if data.get(..17)? != b"Extended Module: "
            || data.get(37) != Some(&0x1a)
            || le16(data, 58)? != 0x0104
        {
            return None;
        }
        let header_size = le32(data, 60)? as usize;
        let orders = le16(data, 64)?;
        let restart = le16(data, 66)?;
        let channels = le16(data, 68)?;
        let patterns = le16(data, 70)?;
        let instruments = le16(data, 72)?;
        if !(276..=4096).contains(&header_size)
            || !(1..=256).contains(&orders)
            || restart >= orders
            || !(1..=32).contains(&channels)
            || !(1..=256).contains(&patterns)
            || instruments > 128
            || le16(data, 74)? > 1
            || !(1..=31).contains(&le16(data, 76)?)
            || !(32..=255).contains(&le16(data, 78)?)
        {
            return None;
        }
        let mut position = 60 + header_size;
        let mut total_cells = 0;
        let mut note_count = 0usize;
        let mut referenced = [false; 129];
        for _ in 0..patterns {
            charge(budget, &mut stopped)?;
            let start = position;
            let header_len = le32(data, start)? as usize;
            let rows = le16(data, start + 5)? as usize;
            let packed_len = le16(data, start + 7)? as usize;
            if !(9..=4096).contains(&header_len)
                || data.get(start + 4) != Some(&0)
                || !(1..=256).contains(&rows)
            {
                return None;
            }
            let cells = rows * usize::from(channels);
            total_cells += cells;
            if total_cells > MAX_CELLS {
                return None;
            }
            position = position.checked_add(header_len)?;
            let packed = data.get(position..position.checked_add(packed_len)?)?;
            position += packed_len;
            if packed.is_empty() {
                continue;
            }
            let mut cursor = 0;
            for _ in 0..cells {
                charge(budget, &mut stopped)?;
                let flag = *packed.get(cursor)?;
                cursor += 1;
                let mut cell = [0u8; 5];
                if flag & 0x80 == 0 {
                    cell[0] = flag;
                    cell[1..].copy_from_slice(packed.get(cursor..cursor + 4)?);
                    cursor += 4;
                } else {
                    if flag & 0x60 != 0 {
                        return None;
                    }
                    for (bit, value) in cell.iter_mut().enumerate() {
                        if flag & (1 << bit) != 0 {
                            *value = *packed.get(cursor)?;
                            cursor += 1;
                        }
                    }
                }
                if cell[0] > 97
                    || u16::from(cell[1]) > instruments
                    || !(cell[2] == 0 || (0x10..=0x50).contains(&cell[2]) || cell[2] >= 0x60)
                {
                    return None;
                }
                note_count += usize::from((1..=96).contains(&cell[0]));
                referenced[usize::from(cell[1])] = true;
            }
            if cursor != packed.len() {
                return None;
            }
        }
        let mut samples = 0u16;
        let mut sample_points = 0u32;
        let mut playable_instrument = false;
        for instrument_index in 1..=instruments {
            charge(budget, &mut stopped)?;
            let start = position;
            let header_len = le32(data, start)? as usize;
            let count = le16(data, start + 27)?;
            if !(29..=4096).contains(&header_len) || count > 16 {
                return None;
            }
            data.get(start..start.checked_add(header_len)?)?;
            position += header_len;
            if count == 0 {
                continue;
            }
            if header_len < 243 {
                return None;
            }
            let sample_header_len = le32(data, start + 29)? as usize;
            if !(40..=4096).contains(&sample_header_len)
                || data
                    .get(start + 33..start + 129)?
                    .iter()
                    .any(|&index| u16::from(index) >= count)
            {
                return None;
            }
            for (points_offset, count_offset, sustain_offset, loop_offset, flags_offset) in
                [(129, 225, 227, 228, 233), (177, 226, 230, 231, 234)]
            {
                let points = usize::from(*data.get(start + count_offset)?);
                let flags = *data.get(start + flags_offset)?;
                if points > 12 || flags & !7 != 0 || (flags & 1 != 0 && points == 0) {
                    return None;
                }
                if flags & 2 != 0 && usize::from(*data.get(start + sustain_offset)?) >= points {
                    return None;
                }
                if flags & 4 != 0 {
                    let a = *data.get(start + loop_offset)?;
                    let b = *data.get(start + loop_offset + 1)?;
                    if a > b || usize::from(b) >= points {
                        return None;
                    }
                }
                let mut previous = None;
                for index in 0..points {
                    let tick = le16(data, start + points_offset + index * 4)?;
                    let value = le16(data, start + points_offset + index * 4 + 2)?;
                    if value > 64 || previous.is_some_and(|value| value >= tick) {
                        return None;
                    }
                    previous = Some(tick);
                }
            }
            if *data.get(start + 235)? > 3 {
                return None;
            }
            let mut byte_count = 0usize;
            for index in 0..usize::from(count) {
                charge(budget, &mut stopped)?;
                let at = position + index * sample_header_len;
                let len = le32(data, at)? as usize;
                let loop_start = le32(data, at + 4)? as usize;
                let loop_len = le32(data, at + 8)? as usize;
                let flags = *data.get(at + 14)?;
                let stride = if flags & 0x10 != 0 { 2 } else { 1 };
                if flags & !0x13 != 0
                    || flags & 3 == 3
                    || *data.get(at + 12)? > 64
                    || !len.is_multiple_of(stride)
                    || !loop_start.is_multiple_of(stride)
                    || !loop_len.is_multiple_of(stride)
                    || (flags & 3 != 0 && loop_len != 0 && loop_start.checked_add(loop_len)? > len)
                {
                    return None;
                }
                byte_count = byte_count.checked_add(len)?;
                sample_points = sample_points.checked_add((len / stride) as u32)?;
                playable_instrument |= referenced[usize::from(instrument_index)] && len != 0;
            }
            position = position.checked_add(usize::from(count) * sample_header_len)?;
            data.get(position..position.checked_add(byte_count)?)?;
            position += byte_count;
            samples += count;
        }
        if note_count == 0 || !playable_instrument || position > super::super::MAX_ROM_BYTES {
            return None;
        }
        Some(EmbeddedModule {
            format: EmbeddedFormat::Xm,
            span: FileSpan {
                offset: base as u32,
                byte_len: position as u32,
            },
            name: text(data.get(17..37)?),
            channels,
            orders,
            patterns,
            instruments,
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

pub(crate) fn mod_channels(data: &[u8]) -> Option<u16> {
    let tag = data.get(..4)?;
    match tag {
        b"M.K." | b"M!K!" | b"FLT4" | b"4CHN" => Some(4),
        [b'1'..=b'9', b'C', b'H', b'N'] => Some(u16::from(tag[0] - b'0')),
        [b'1'..=b'3', b'0'..=b'9', b'C', b'H'] => {
            let channels = u16::from(tag[0] - b'0') * 10 + u16::from(tag[1] - b'0');
            (channels <= 32).then_some(channels)
        }
        _ => None,
    }
}

pub(crate) fn mod_file(
    bytes: &[u8],
    base: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<EmbeddedModule>, ScanStop> {
    let mut stopped = None;
    let result = (|| {
        let data = bytes.get(base..)?;
        let channels = mod_channels(data.get(1080..)?)?;
        let orders = *data.get(950)?;
        if !(1..=128).contains(&orders) {
            return None;
        }
        let restart = *data.get(951)?;
        if restart >= orders && !matches!(restart, 0x78 | 0x7f) {
            return None;
        }
        let order_data = data.get(952..1080)?;
        if order_data.iter().any(|value| *value > 127) {
            return None;
        }
        let patterns = u16::from(*order_data.iter().max()?) + 1;
        let mut lengths = [0usize; 32];
        let mut sample_points = 0usize;
        let mut samples = 0;
        for index in 0..31 {
            charge(budget, &mut stopped)?;
            let at = 20 + index * 30;
            let length = usize::from(be16(data, at + 22)?) * 2;
            let loop_start = usize::from(be16(data, at + 26)?) * 2;
            let loop_length = usize::from(be16(data, at + 28)?) * 2;
            if *data.get(at + 24)? > 15
                || *data.get(at + 25)? > 64
                || (loop_length > 2 && loop_start.checked_add(loop_length)? > length)
            {
                return None;
            }
            lengths[index + 1] = length;
            sample_points += length;
            samples += u16::from(length != 0);
        }
        let cells = usize::from(patterns) * 64 * usize::from(channels);
        if cells > MAX_CELLS {
            return None;
        }
        let pattern_end = 1084 + cells * 4;
        let pattern_data = data.get(1084..pattern_end)?;
        let mut pitched_note = false;
        let mut referenced_sample = false;
        for cell in pattern_data.as_chunks::<4>().0 {
            charge(budget, &mut stopped)?;
            let instrument = usize::from((cell[0] & 0xf0) | (cell[2] >> 4));
            let period = u16::from(cell[0] & 15) * 256 + u16::from(cell[1]);
            if instrument > 31 || (period != 0 && period < 14) {
                return None;
            }
            pitched_note |= period != 0;
            referenced_sample |= lengths[instrument] != 0;
        }
        let length = pattern_end.checked_add(sample_points)?;
        data.get(..length)?;
        if !pitched_note || !referenced_sample || length > super::super::MAX_ROM_BYTES {
            return None;
        }
        Some(EmbeddedModule {
            format: EmbeddedFormat::Mod,
            span: FileSpan {
                offset: base as u32,
                byte_len: length as u32,
            },
            name: text(data.get(..20)?),
            channels,
            orders: u16::from(orders),
            patterns,
            instruments: 31,
            samples,
            sample_points: sample_points as u32,
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

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

pub(crate) fn text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|&byte| {
            if byte.is_ascii_graphic() || byte == b' ' {
                char::from(byte)
            } else {
                '?'
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}
