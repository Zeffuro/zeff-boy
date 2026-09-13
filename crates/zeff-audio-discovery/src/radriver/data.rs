use std::collections::BTreeSet;

use super::{
    Budget, MAX_ORDERS, MAX_TABLE_ENTRIES, RadriverLayout, RadriverNativeProfile, RadriverSample,
    RadriverSong, RadriverSongKind, ReadError, ReadResult, ScanStop, half, pointer, span, word,
};

const PCM_PADDING: usize = 768;

pub(super) fn parse_song(
    bytes: &[u8],
    native: &RadriverNativeProfile,
    kind: RadriverSongKind,
    index: u16,
    budget: &mut Budget<'_>,
) -> ReadResult<RadriverSong> {
    let (root, header, order, blocks, samples) = match kind {
        RadriverSongKind::Effect => {
            let bank = native.bank.effective_offset as usize;
            let count = word(bytes, bank + 4).ok_or(ReadError::Invalid)? as usize;
            if usize::from(index) >= count || count > MAX_TABLE_ENTRIES {
                return Err(ReadError::Invalid);
            }
            let table = pointer(
                bytes,
                word(bytes, bank + 12).ok_or(ReadError::Invalid)?,
                count * 4,
            )?;
            let header = span(
                bytes,
                table.effective_offset as usize + usize::from(index) * 4,
                4,
            )?;
            let sample = sample(
                bytes,
                word(bytes, header.effective_offset as usize).unwrap(),
                budget,
            )?;
            if sample.encoding == 0 && native.layout == RadriverLayout::GlobalState {
                return Err(ReadError::Invalid);
            }
            (native.bank, header, None, Some(table), vec![sample])
        }
        RadriverSongKind::CompressedMusic => {
            let root = native.music_table.ok_or(ReadError::Invalid)?;
            if usize::from(index) >= root.byte_len as usize / 8 {
                return Err(ReadError::Invalid);
            }
            let header = span(
                bytes,
                root.effective_offset as usize + usize::from(index) * 8,
                8,
            )?;
            let table = word(bytes, header.effective_offset as usize).ok_or(ReadError::Invalid)?;
            let order_address =
                word(bytes, header.effective_offset as usize + 4).ok_or(ReadError::Invalid)?;
            let order_offset = order_address
                .checked_sub(0x0800_0000)
                .ok_or(ReadError::Invalid)? as usize;
            if !order_offset.is_multiple_of(2) {
                return Err(ReadError::Invalid);
            }
            let mut used = BTreeSet::new();
            let mut ended = None;
            for slot in 0..MAX_ORDERS {
                budget.charge()?;
                let value = half(bytes, order_offset + slot * 2).ok_or(ReadError::Invalid)? as i16;
                if value == -1 {
                    if slot == 0 {
                        return Err(ReadError::Invalid);
                    }
                    ended = Some(slot + 1);
                    break;
                }
                if value < 0 || value as usize >= MAX_TABLE_ENTRIES {
                    return Err(ReadError::Invalid);
                }
                used.insert(value as usize);
            }
            let length = ended.ok_or(ReadError::Stop(ScanStop::ValidationLimit))?;
            let order = span(bytes, order_offset, length * 2)?;
            let count = used.last().copied().ok_or(ReadError::Invalid)? + 1;
            let blocks = pointer(bytes, table, count * 4)?;
            let mut samples = Vec::new();
            let mut addresses = BTreeSet::new();
            samples
                .try_reserve_exact(used.len())
                .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
            for slot in used {
                budget.charge()?;
                let address = word(bytes, blocks.effective_offset as usize + slot * 4)
                    .ok_or(ReadError::Invalid)?;
                if !addresses.insert(address) {
                    continue;
                }
                let sample = sample(bytes, address, budget)?;
                if sample.encoding != 0 || sample.loop_start.is_some() {
                    return Err(ReadError::Invalid);
                }
                // A native transition renders the rest of this batch from the next block.
                if sample.data.byte_len < 144 {
                    return Err(ReadError::Invalid);
                }
                samples.push(sample);
            }
            (root, header, Some(order), Some(blocks), samples)
        }
    };
    let mut mapped = native.setup_spans.clone();
    mapped.extend([root, header, native.bank]);
    mapped.extend(order);
    mapped.extend(blocks);
    for sample in &samples {
        mapped.extend([sample.header, sample.data]);
        mapped.extend(sample.padding);
    }
    mapped.sort_unstable();
    mapped.dedup();
    Ok(RadriverSong {
        root,
        header,
        index,
        kind,
        title: format!(
            "RADriver {} {index}",
            if kind == RadriverSongKind::Effect {
                "effect"
            } else {
                "music"
            }
        ),
        order,
        blocks,
        samples,
        native: native.clone(),
        mapped_spans: mapped,
        warnings: Vec::new(),
    })
}

fn sample(bytes: &[u8], address: u32, budget: &mut Budget<'_>) -> ReadResult<RadriverSample> {
    budget.charge()?;
    let header = pointer(bytes, address, 16)?;
    let at = header.effective_offset as usize;
    let encoding = word(bytes, at).ok_or(ReadError::Invalid)?;
    let length = word(bytes, at + 4).ok_or(ReadError::Invalid)? as usize;
    let loop_start = word(bytes, at + 8).ok_or(ReadError::Invalid)?;
    if encoding > 1
        || length == 0
        || loop_start as usize >= length
        || word(bytes, at + 12) != Some(0)
    {
        return Err(ReadError::Invalid);
    }
    let data = span(bytes, at + 16, length)?;
    let padding = if encoding == 1 {
        let padding = span(bytes, at + 16 + length, PCM_PADDING)?;
        // The native PCM mixer reads past the endpoint before retiring a channel.
        for offset in 0..PCM_PADDING {
            budget.charge()?;
            let expected = if loop_start == 0 {
                0
            } else {
                bytes[at + 16 + loop_start as usize + offset % (length - loop_start as usize)]
            };
            if bytes[padding.effective_offset as usize + offset] != expected {
                return Err(ReadError::Invalid);
            }
        }
        Some(padding)
    } else {
        None
    };
    Ok(RadriverSample {
        header,
        data,
        padding,
        encoding: encoding as u8,
        loop_start: (loop_start != 0).then_some(loop_start),
    })
}
