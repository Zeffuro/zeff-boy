use super::wod;
use super::{
    Budget, MAX_EVENTS, MAX_TABLE_ENTRIES, NsqNativeProfile, NsqSong, ReadError, ReadResult,
    RomSpan, ScanStop, half, span, word,
};

const ENTRY_BYTES: usize = 8;
const SENTINEL: u32 = u32::MAX;
const NPF_BYTES: usize = 128 * 48;
const NPF_ENTRY_BYTES: usize = 48;
const MAX_TABLES: usize = 16;

struct TableEntry {
    selector: u16,
    name: String,
    name_span: RomSpan,
    asset: wod::Asset,
}

pub(super) fn parse_song(
    bytes: &[u8],
    native: &NsqNativeProfile,
    slot: usize,
    budget: &mut Budget<'_>,
) -> ReadResult<NsqSong> {
    let entries = table_entries(bytes, native.song_table, native.filesystem, budget)?;
    let selected = entries.get(slot).ok_or(ReadError::Invalid)?;
    let header = span(
        bytes,
        native.song_table.effective_offset as usize + slot * ENTRY_BYTES,
        ENTRY_BYTES,
    )?;
    let root = native.song_table;
    let bank = bank(bytes, native, budget)?;
    let (prefix, prefix_span) = checked_path(bytes, native.bank_directory)?;
    if prefix.ends_with('\\') {
        return Err(ReadError::Invalid);
    }
    let nsq_header = span(bytes, selected.asset.span.effective_offset as usize, 8)?;
    let events = span(
        bytes,
        selected.asset.span.effective_offset as usize + 8,
        selected.asset.span.byte_len as usize - 8,
    )?;
    let sequence = selected.asset.span;
    let (notes, duration_frames, used) = sequence_info(bytes, nsq_header, events, budget)?;
    let mut raw_assets = Vec::new();
    raw_assets
        .try_reserve(256)
        .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
    let (release_prefix, release_span) = checked_path(bytes, native.release_prefix)?;
    let attack_prefix = format!("{prefix}\\midi");
    // The native bank loader initializes every patch before any song is selected.
    for instrument in 0..128 {
        budget.charge()?;
        let entry = bank.effective_offset as usize + instrument * NPF_ENTRY_BYTES;
        let (sample, release) = patch(bytes, entry)?;
        if let Some(sample) = sample {
            add_sample(
                bytes,
                native.filesystem,
                &attack_prefix,
                sample,
                &mut raw_assets,
                budget,
            )?;
        }
        if let Some(release) = release {
            add_sample(
                bytes,
                native.filesystem,
                &release_prefix,
                release,
                &mut raw_assets,
                budget,
            )?;
        }
    }
    let mut mapped = Vec::new();
    mapped
        .try_reserve(10 + raw_assets.len() + native.setup_spans.len())
        .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
    mapped.extend([
        root,
        header,
        sequence,
        bank,
        native.filesystem,
        selected.name_span,
        prefix_span,
        native.instrument_path,
        release_span,
    ]);
    mapped.extend(raw_assets.iter().copied());
    mapped.extend(native.setup_spans.iter().copied());
    mapped.sort_unstable();
    mapped.dedup();
    let title = selected
        .name
        .rsplit('\\')
        .next()
        .and_then(|name| name.strip_suffix(".nsq"))
        .filter(|name| !name.is_empty())
        .ok_or(ReadError::Invalid)?
        .to_owned();
    Ok(NsqSong {
        root,
        header,
        sequence,
        bank,
        index: selected.selector,
        slot: u16::try_from(slot).map_err(|_| ReadError::Invalid)?,
        title,
        notes,
        duration_frames,
        instruments: used.iter().filter(|used| **used).count() as u16,
        samples: raw_assets.len() as u16,
        native: native.clone(),
        mapped_spans: mapped,
        warnings: vec![
            "Runs the original NSQ driver and its native raw-sample codec.".to_owned(),
            "Sample inventory includes every sample loaded by the bank during startup.".to_owned(),
        ],
    })
}

pub(super) fn scan_tables(
    bytes: &[u8],
    filesystem: RomSpan,
    budget: &mut Budget<'_>,
) -> ReadResult<Vec<RomSpan>> {
    let mut tables = Vec::new();
    for offset in (0..bytes.len().saturating_sub(ENTRY_BYTES - 1)).step_by(4) {
        budget.charge()?;
        if !valid_entry(bytes, offset, filesystem, budget)?
            || (offset >= ENTRY_BYTES
                && valid_entry(bytes, offset - ENTRY_BYTES, filesystem, budget)?)
        {
            continue;
        }
        let Some(table) = table_at(bytes, offset, filesystem, budget)? else {
            continue;
        };
        if tables.len() == MAX_TABLES {
            return Err(ReadError::Stop(ScanStop::ValidationLimit));
        }
        tables
            .try_reserve(1)
            .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
        tables.push(table);
    }
    Ok(tables)
}

fn table_entries(
    bytes: &[u8],
    table: RomSpan,
    filesystem: RomSpan,
    budget: &mut Budget<'_>,
) -> ReadResult<Vec<TableEntry>> {
    let offset = table.effective_offset as usize;
    let length = table.byte_len as usize;
    if length < ENTRY_BYTES * 2
        || !length.is_multiple_of(ENTRY_BYTES)
        || table != span(bytes, offset, length)?
    {
        return Err(ReadError::Invalid);
    }
    let count = length / ENTRY_BYTES - 1;
    if !(1..=MAX_TABLE_ENTRIES).contains(&count)
        || word(bytes, offset + count * ENTRY_BYTES) != Some(SENTINEL)
        || word(bytes, offset + count * ENTRY_BYTES + 4) != Some(0)
    {
        return Err(ReadError::Invalid);
    }
    let mut seen = Vec::new();
    let mut entries = Vec::new();
    seen.try_reserve(count)
        .and_then(|_| entries.try_reserve(count))
        .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
    for slot in 0..count {
        let entry = entry(bytes, offset + slot * ENTRY_BYTES, filesystem, budget)?;
        match seen.binary_search(&u32::from(entry.selector)) {
            Ok(_) => return Err(ReadError::Invalid),
            Err(index) => seen.insert(index, u32::from(entry.selector)),
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn table_at(
    bytes: &[u8],
    offset: usize,
    filesystem: RomSpan,
    budget: &mut Budget<'_>,
) -> ReadResult<Option<RomSpan>> {
    let mut seen = Vec::new();
    seen.try_reserve(MAX_TABLE_ENTRIES)
        .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
    for count in 0..=MAX_TABLE_ENTRIES {
        budget.charge()?;
        let at = offset
            .checked_add(count * ENTRY_BYTES)
            .ok_or(ReadError::Invalid)?;
        let selector = word(bytes, at);
        let pointer = word(bytes, at + 4);
        if selector == Some(SENTINEL) && pointer == Some(0) {
            return if count == 0 {
                Ok(None)
            } else {
                Ok(Some(span(bytes, offset, (count + 1) * ENTRY_BYTES)?))
            };
        }
        if count == MAX_TABLE_ENTRIES {
            return Ok(None);
        }
        let entry = match entry(bytes, at, filesystem, budget) {
            Ok(entry) => entry,
            Err(ReadError::Invalid) => return Ok(None),
            Err(ReadError::Stop(stop)) => return Err(ReadError::Stop(stop)),
        };
        let selector = u32::from(entry.selector);
        if seen.binary_search(&selector).is_ok() {
            return Ok(None);
        }
        let index = seen.binary_search(&selector).unwrap_err();
        seen.insert(index, selector);
    }
    Ok(None)
}

fn valid_entry(
    bytes: &[u8],
    offset: usize,
    filesystem: RomSpan,
    budget: &mut Budget<'_>,
) -> ReadResult<bool> {
    match entry(bytes, offset, filesystem, budget) {
        Ok(_) => Ok(true),
        Err(ReadError::Invalid) => Ok(false),
        Err(ReadError::Stop(stop)) => Err(ReadError::Stop(stop)),
    }
}

fn entry(
    bytes: &[u8],
    offset: usize,
    filesystem: RomSpan,
    budget: &mut Budget<'_>,
) -> ReadResult<TableEntry> {
    budget.charge()?;
    let selector = word(bytes, offset).ok_or(ReadError::Invalid)?;
    if selector == SENTINEL || selector > u32::from(u16::MAX) {
        return Err(ReadError::Invalid);
    }
    let pointer = wod::pointer_offset(bytes, word(bytes, offset + 4).ok_or(ReadError::Invalid)?)?;
    let (name, name_span) = wod::path(bytes, pointer)?;
    if !name.ends_with(".nsq") {
        return Err(ReadError::Invalid);
    }
    let asset = wod::resolve_asset(bytes, filesystem, &name, budget)?;
    if asset.flags != 0
        || asset.span.byte_len < 24
        || bytes.get(asset.span.effective_offset as usize..asset.span.effective_offset as usize + 4)
            != Some(b"NSQ\0")
    {
        return Err(ReadError::Invalid);
    }
    Ok(TableEntry {
        selector: selector as u16,
        name,
        name_span,
        asset,
    })
}

fn checked_path(bytes: &[u8], expected: RomSpan) -> ReadResult<(String, RomSpan)> {
    let (name, span_value) = wod::path(bytes, expected.effective_offset as usize)?;
    if span_value != expected {
        return Err(ReadError::Invalid);
    }
    Ok((name, span_value))
}

fn bank(bytes: &[u8], native: &NsqNativeProfile, budget: &mut Budget<'_>) -> ReadResult<RomSpan> {
    let (name, path_span) = checked_path(bytes, native.instrument_path)?;
    if path_span != native.instrument_path || !name.ends_with("instruments.npf") {
        return Err(ReadError::Invalid);
    }
    let asset = wod::resolve_asset(bytes, native.filesystem, &name, budget)?;
    if asset.flags != 0 || asset.span.byte_len != NPF_BYTES as u32 {
        return Err(ReadError::Invalid);
    }
    Ok(asset.span)
}

fn sequence_info(
    bytes: &[u8],
    header: RomSpan,
    sequence: RomSpan,
    budget: &mut Budget<'_>,
) -> ReadResult<(u32, u32, [bool; 128])> {
    let start = header.effective_offset as usize;
    if bytes.get(start..start + 4) != Some(b"NSQ\0")
        || sequence.effective_offset as usize != start + 8
    {
        return Err(ReadError::Invalid);
    }
    let length = sequence.byte_len as usize;
    if length == 0 || !length.is_multiple_of(16) {
        return Err(ReadError::Invalid);
    }
    let records = length / 16;
    if records > MAX_EVENTS {
        return Err(ReadError::Invalid);
    }
    let mut notes = 0u32;
    let mut duration = 0u32;
    let mut previous_tick = 0u16;
    let mut used = [false; 128];
    for index in 0..records {
        budget.charge()?;
        let at = sequence.effective_offset as usize + index * 16;
        let tick = half(bytes, at).ok_or(ReadError::Invalid)?;
        if index != 0 && tick < previous_tick {
            return Err(ReadError::Invalid);
        }
        previous_tick = tick;
        let opcode = *bytes.get(at + 2).ok_or(ReadError::Invalid)?;
        let instrument = *bytes.get(at + 3).ok_or(ReadError::Invalid)?;
        let note = *bytes.get(at + 4).ok_or(ReadError::Invalid)?;
        match opcode {
            9 => {
                if instrument >= 128 || note >= 128 {
                    return Err(ReadError::Invalid);
                }
                used[instrument as usize] = true;
                notes = notes.checked_add(1).ok_or(ReadError::Invalid)?;
            }
            8 => {
                if instrument >= 128 || note >= 128 {
                    return Err(ReadError::Invalid);
                }
            }
            47 if index + 1 == records => duration = u32::from(tick) + 1,
            _ => return Err(ReadError::Invalid),
        }
    }
    if duration == 0 {
        return Err(ReadError::Invalid);
    }
    Ok((notes, duration, used))
}

fn patch(bytes: &[u8], offset: usize) -> ReadResult<(Option<i32>, Option<i32>)> {
    let loop_start = word(bytes, offset).ok_or(ReadError::Invalid)? as i32;
    let sample = word(bytes, offset + 4).ok_or(ReadError::Invalid)? as i32;
    let release = word(bytes, offset + 8).ok_or(ReadError::Invalid)? as i32;
    let pitch = word(bytes, offset + 12).ok_or(ReadError::Invalid)? as i32;
    let psg_shape = word(bytes, offset + 44).ok_or(ReadError::Invalid)?;
    if loop_start < -1 || sample < -2 || release < -1 || pitch < 0 {
        return Err(ReadError::Invalid);
    }
    match sample {
        -2 if loop_start == 0 && psg_shape != 0 => Ok((None, None)),
        -1 if loop_start == 0 && psg_shape == 0 => Ok((None, None)),
        -2 | -1 => Err(ReadError::Invalid),
        _ if psg_shape != 0 || sample >= 128 || release >= 128 => Err(ReadError::Invalid),
        _ => Ok((Some(sample), (release >= 0).then_some(release))),
    }
}

fn add_sample(
    bytes: &[u8],
    filesystem: RomSpan,
    prefix: &str,
    id: i32,
    assets: &mut Vec<RomSpan>,
    budget: &mut Budget<'_>,
) -> ReadResult<()> {
    if id < 0 {
        return Ok(());
    }
    let name = format!("{prefix}{id}.raw");
    let asset = wod::resolve_asset(bytes, filesystem, &name, budget)?;
    if asset.flags != 0 || asset.span.byte_len == 0 {
        return Err(ReadError::Invalid);
    }
    if !assets.contains(&asset.span) {
        if assets.len() == 256 {
            return Err(ReadError::Stop(ScanStop::ValidationLimit));
        }
        assets
            .try_reserve(1)
            .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
        assets.push(asset.span);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
