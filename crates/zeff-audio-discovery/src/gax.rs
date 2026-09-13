use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use serde::Serialize;

use super::{Budget, RomSpan, ScanStop, rom_pointer, word};

mod instruments;
use instruments::read_instrument;

const HEADER_LEN: usize = 200;
const MAX_CELLS: usize = 262_144;
const MAX_PATTERNS: usize = 512;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GaxSong {
    pub header: RomSpan,
    pub version: String,
    pub version_span: RomSpan,
    pub title: String,
    pub artist: String,
    pub metadata: RomSpan,
    pub rows_per_pattern: u16,
    pub restart_order: u16,
    pub volume: u16,
    pub sample_rate: u16,
    pub channels: Vec<GaxChannel>,
    pub patterns: Vec<GaxPattern>,
    pub instruments: Vec<GaxInstrument>,
    pub samples: Vec<GaxSample>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<GaxWarning>,
    pub projection_limitations: Vec<String>,
    pub xm_exportable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GaxChannel {
    pub order_table: RomSpan,
    pub orders: Vec<GaxOrder>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GaxOrder {
    pub pattern: u16,
    pub transpose: i8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GaxPattern {
    pub source: RomSpan,
    pub rows: Vec<GaxCell>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GaxCell {
    /// GAX 0: instrument only, 1: note off, 2..127: pitched note.
    pub note: Option<u8>,
    pub instrument: Option<u8>,
    pub effect: u8,
    pub parameter: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GaxInstrument {
    pub index: u8,
    pub blank: bool,
    pub pointer: RomSpan,
    pub descriptor: RomSpan,
    pub sample_indices: [u8; 4],
    pub vibrato: [u8; 3],
    pub row_speed: u8,
    pub rows_span: RomSpan,
    pub rows: Vec<GaxInstrumentRow>,
    pub sample_settings: Vec<GaxSampleSettings>,
    pub envelope: GaxEnvelope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GaxInstrumentRow {
    pub relative_note: u8,
    pub fixed_note: bool,
    pub sample_slot: u8,
    pub effects: [[u8; 2]; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GaxSampleSettings {
    pub source: RomSpan,
    /// Pitch in 1/32 semitone units, as stored by GAX 3.
    pub pitch: i16,
    pub bidirectional: bool,
    pub start_position: i32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub modulation: u8,
    pub unknown_10: i32,
    pub modulation_timer: u16,
    pub unknown_16: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GaxEnvelope {
    pub source: RomSpan,
    pub sustain: Option<u8>,
    pub loop_start: Option<u8>,
    pub loop_end: Option<u8>,
    pub points: Vec<GaxEnvelopePoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GaxEnvelopePoint {
    pub tick: u16,
    pub interpolation: i16,
    pub volume: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GaxSample {
    pub index: u8,
    pub header: RomSpan,
    /// GAX 3 unsigned 8-bit PCM.
    pub data: RomSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct GaxWarning {
    pub offset: u32,
    pub reason: String,
}

#[derive(Debug)]
enum ReadError {
    Invalid,
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(stop: ScanStop) -> Self {
        Self::Stop(stop)
    }
}

type ReadResult<T> = Result<T, ReadError>;

fn need<T>(value: Option<T>) -> ReadResult<T> {
    value.ok_or(ReadError::Invalid)
}
fn check(valid: bool) -> ReadResult<()> {
    valid.then_some(()).ok_or(ReadError::Invalid)
}
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn mapped(bytes: &[u8], at: usize, len: usize, alignment: usize) -> ReadResult<usize> {
    need(rom_pointer(bytes, need(word(bytes, at))?, len, alignment))
}
fn warning(warnings: &mut Vec<GaxWarning>, offset: usize, reason: &str) {
    if warnings.iter().any(|warning| warning.reason == reason) {
        return;
    }
    warnings.push(GaxWarning {
        offset: offset as u32,
        reason: reason.into(),
    });
}

pub(crate) fn scan(
    bytes: &[u8],
    output: &mut Vec<GaxSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    scan_with_inventory_limit(bytes, output, budget, max_candidates, MAX_RETAINED_BYTES)
}

fn scan_with_inventory_limit(
    bytes: &[u8],
    output: &mut Vec<GaxSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    retained_limit: usize,
) -> Result<(), ScanStop> {
    let versions = find_versions(bytes, budget)?;
    if versions.is_empty() {
        return Ok(());
    }
    let (version_at, version, _) = &versions[0];
    let ambiguous = versions
        .iter()
        .any(|(_, text, major)| *major != 3 || text != version);
    let binding = ambiguous
        .then(|| super::gax_native::v3::VersionBinding::recognize(bytes, budget))
        .transpose()?;
    output
        .try_reserve_exact(max_candidates.saturating_sub(output.len()))
        .map_err(|_| ScanStop::InventoryLimit)?;
    let mut retained_bytes = retained_owned_bytes(output);
    if retained_bytes > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    let version_span = RomSpan::new(*version_at, version.chars().count() + 1);
    for at in (0..=bytes.len().saturating_sub(HEADER_LEN)).step_by(4) {
        budget.charge()?;
        let Some(channels @ 1..=32) = u16_at(bytes, at) else {
            continue;
        };
        if !matches!(u16_at(bytes, at + 2), Some(1..=511))
            || !matches!(u16_at(bytes, at + 4), Some(1..=255))
        {
            continue;
        }
        if word(bytes, at + 32 + usize::from(channels - 1) * 4)
            .is_none_or(|p| !(0x0800_0000..0x0A00_0000).contains(&p))
        {
            continue;
        }
        let (version, version_span) = if let Some(binding) = &binding {
            let Some(version) = binding.at(at) else {
                continue;
            };
            version
        } else {
            (version.as_str(), version_span)
        };
        match read_song(bytes, at, version, version_span, budget) {
            Ok(song) => {
                if output.len() >= max_candidates {
                    return Err(ScanStop::CandidateLimit);
                }
                retained_bytes = retained_bytes
                    .checked_add(song_owned_bytes(&song))
                    .filter(|bytes| *bytes <= retained_limit)
                    .ok_or(ScanStop::InventoryLimit)?;
                output.push(song);
            }
            Err(ReadError::Invalid) => (),
            Err(ReadError::Stop(stop)) => return Err(stop),
        }
    }
    Ok(())
}

fn retained_owned_bytes(songs: &Vec<GaxSong>) -> usize {
    songs
        .capacity()
        .saturating_mul(size_of::<GaxSong>())
        .saturating_add(
            songs
                .iter()
                .map(song_owned_bytes)
                .fold(0usize, usize::saturating_add),
        )
}

fn song_owned_bytes(song: &GaxSong) -> usize {
    let mut bytes = song
        .version
        .capacity()
        .saturating_add(song.title.capacity())
        .saturating_add(song.artist.capacity())
        .saturating_add(capacity_bytes(&song.channels))
        .saturating_add(capacity_bytes(&song.patterns))
        .saturating_add(capacity_bytes(&song.instruments))
        .saturating_add(capacity_bytes(&song.samples))
        .saturating_add(capacity_bytes(&song.mapped_spans))
        .saturating_add(capacity_bytes(&song.warnings))
        .saturating_add(capacity_bytes(&song.projection_limitations));
    for channel in &song.channels {
        bytes = bytes.saturating_add(capacity_bytes(&channel.orders));
    }
    for pattern in &song.patterns {
        bytes = bytes.saturating_add(capacity_bytes(&pattern.rows));
    }
    for instrument in &song.instruments {
        bytes = bytes
            .saturating_add(capacity_bytes(&instrument.rows))
            .saturating_add(capacity_bytes(&instrument.sample_settings))
            .saturating_add(capacity_bytes(&instrument.envelope.points));
    }
    for warning in &song.warnings {
        bytes = bytes.saturating_add(warning.reason.capacity());
    }
    for limitation in &song.projection_limitations {
        bytes = bytes.saturating_add(limitation.capacity());
    }
    bytes
}

fn capacity_bytes<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(size_of::<T>())
}

fn find_versions(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<(usize, String, u8)>, ScanStop> {
    const PREFIX: &[u8] = b"GAX Sound Engine ";
    let mut result = Vec::new();
    for at in 0..bytes.len().saturating_sub(PREFIX.len()) {
        if at % 16 == 0 {
            budget.charge()?;
        }
        if !bytes[at..].starts_with(PREFIX) {
            continue;
        }
        let tail = &bytes[at..bytes.len().min(at + 256)];
        let Some(end) = tail.iter().position(|b| *b == 0) else {
            continue;
        };
        let text = &tail[..end];
        if !text.contains(&0xA9) {
            continue;
        }
        let mut p = PREFIX.len();
        if matches!(text.get(p), Some(b'v' | b'V')) {
            p += 1;
        }
        let Some(&major @ b'0'..=b'9') = text.get(p) else {
            continue;
        };
        if text.get(p + 1) != Some(&b'.') || !text.get(p + 2).is_some_and(u8::is_ascii_digit) {
            continue;
        }
        if result.len() == 16 {
            return Err(ScanStop::InventoryLimit);
        }
        result.push((
            at,
            text.iter().map(|b| char::from(*b)).collect(),
            major - b'0',
        ));
    }
    Ok(result)
}

fn read_song(
    bytes: &[u8],
    at: usize,
    version: &str,
    version_span: RomSpan,
    budget: &mut Budget<'_>,
) -> ReadResult<GaxSong> {
    need(bytes.get(at..at + HEADER_LEN))?;
    let channel_count = usize::from(need(u16_at(bytes, at))?);
    let rows = usize::from(need(u16_at(bytes, at + 2))?);
    let orders = usize::from(need(u16_at(bytes, at + 4))?);
    check(
        (1..=32).contains(&channel_count)
            && (1..=511).contains(&rows)
            && (1..=255).contains(&orders),
    )?;
    check(channel_count * rows * orders <= MAX_CELLS)?;
    let restart = need(u16_at(bytes, at + 6))?;
    let volume = need(u16_at(bytes, at + 8))?;
    let sample_rate = need(u16_at(bytes, at + 24))?;
    check(usize::from(restart) < orders && (1_000..=65_535).contains(&sample_rate))?;
    check(bytes[at + 28] <= 32)?;
    let sequence = mapped(bytes, at + 12, 1, 1)?;
    let instrument_table = mapped(bytes, at + 16, 4, 4)?;
    let sample_table = mapped(bytes, at + 20, 8, 4)?;
    let mut channels = Vec::with_capacity(channel_count);
    let mut patterns = Vec::new();
    let mut pattern_map = BTreeMap::new();
    let mut used_instruments = BTreeSet::new();
    let mut warnings = Vec::new();
    if volume > 256 {
        warning(
            &mut warnings,
            at + 8,
            "GAX mixing amplification exceeds the supported XM projection range",
        );
    }
    let mut spans = vec![RomSpan::new(at, HEADER_LEN), version_span];
    let mut pitched_notes = 0;
    for channel in 0..32 {
        budget.charge()?;
        if channel >= channel_count {
            check(need(word(bytes, at + 32 + channel * 4))? == 0)?;
            continue;
        }
        let table = mapped(bytes, at + 32 + channel * 4, orders * 4, 4)?;
        let table_span = RomSpan::new(table, orders * 4);
        spans.push(table_span);
        let mut channel_orders = Vec::with_capacity(orders);
        for order in 0..orders {
            budget.charge()?;
            let entry = table + order * 4;
            let offset = sequence + usize::from(need(u16_at(bytes, entry))?);
            check(bytes[entry + 3] == 0)?;
            let pattern = if let Some(&index) = pattern_map.get(&offset) {
                index
            } else {
                if patterns.len() == MAX_PATTERNS {
                    return Err(ReadError::Stop(ScanStop::ValidationLimit));
                }
                let pattern = read_pattern(bytes, offset, rows, budget, &mut warnings)?;
                for row in &pattern.rows {
                    if let Some(note) = row.note {
                        if let Some(index @ 1..=255) = row.instrument {
                            used_instruments.insert(index);
                        }
                        if note > 1 {
                            pitched_notes += 1;
                        }
                    }
                }
                spans.push(pattern.source);
                let index = patterns.len() as u16;
                patterns.push(pattern);
                pattern_map.insert(offset, index);
                index
            };
            channel_orders.push(GaxOrder {
                pattern,
                transpose: bytes[entry + 2] as i8,
            });
        }
        channels.push(GaxChannel {
            order_table: table_span,
            orders: channel_orders,
        });
    }
    check(pitched_notes > 0 && !used_instruments.is_empty())?;
    let name_at = need(
        patterns
            .iter()
            .map(|p| p.source.effective_offset as usize + p.source.byte_len as usize)
            .max(),
    )?;
    let name_end = need(
        channels
            .iter()
            .map(|c| c.order_table.effective_offset as usize)
            .min(),
    )?;
    let (title, artist, metadata) = read_title(bytes, name_at, name_end, budget)?;
    spans.push(metadata);
    let mut instruments = Vec::new();
    let mut sample_indices = BTreeSet::new();
    for index in used_instruments {
        budget.charge()?;
        let instrument = read_instrument(bytes, instrument_table, index, budget, &mut warnings)?;
        spans.extend([
            instrument.pointer,
            instrument.descriptor,
            instrument.rows_span,
            instrument.envelope.source,
        ]);
        for (slot, setting) in instrument.sample_settings.iter().enumerate() {
            sample_indices.insert(instrument.sample_indices[slot]);
            spans.push(setting.source);
        }
        instruments.push(instrument);
    }
    let mut samples = Vec::new();
    let mut total_samples = 0usize;
    for index in sample_indices {
        budget.charge()?;
        let entry = sample_table + usize::from(index) * 8;
        let len = need(word(bytes, entry + 4))? as usize;
        check(len > 0 && len <= super::MAX_ROM_BYTES)?;
        let data = mapped(bytes, entry, len, 1)?;
        total_samples += len;
        check(total_samples <= super::MAX_ROM_BYTES)?;
        let sample = GaxSample {
            index,
            header: RomSpan::new(entry, 8),
            data: RomSpan::new(data, len),
        };
        spans.extend([sample.header, sample.data]);
        samples.push(sample);
    }
    for instrument in &instruments {
        for (slot, setting) in instrument.sample_settings.iter().enumerate() {
            let sample = need(
                samples
                    .iter()
                    .find(|s| s.index == instrument.sample_indices[slot]),
            )?;
            check(
                setting.start_position >= 0
                    && (setting.start_position as u32) < sample.data.byte_len,
            )?;
            if setting.loop_start != 0 || setting.loop_end != 0 {
                check(
                    setting.loop_start <= setting.loop_end
                        && setting.loop_end <= sample.data.byte_len,
                )?;
            }
        }
    }
    if rows > 256 {
        warning(
            &mut warnings,
            at + 2,
            "GAX pattern exceeds XM's 256-row limit",
        );
    }
    if instruments.len() > 128 {
        warning(
            &mut warnings,
            instrument_table,
            "GAX song exceeds XM's 128-instrument limit",
        );
    }
    for channel in &channels {
        let mut active = None;
        for order in &channel.orders {
            for cell in &patterns[usize::from(order.pattern)].rows {
                budget.charge()?;
                if let Some(index @ 1..=255) = cell.instrument {
                    active = Some(index);
                }
                if let Some(note @ 2..=127) = cell.note {
                    let Some(instrument) = instruments.iter().find(|i| Some(i.index) == active)
                    else {
                        warning(
                            &mut warnings,
                            channel.order_table.effective_offset as usize,
                            "GAX note has no initial instrument context",
                        );
                        continue;
                    };
                    let row = &instrument.rows[0];
                    if let Some(setting) = row
                        .sample_slot
                        .checked_sub(1)
                        .and_then(|slot| instrument.sample_settings.get(usize::from(slot)))
                    {
                        let relative = setting.pitch / 32 - 1
                            + if row.fixed_note {
                                0
                            } else {
                                i16::from(row.relative_note) - 2
                            };
                        if i8::try_from(relative).is_err() {
                            warning(
                                &mut warnings,
                                setting.source.effective_offset as usize,
                                "GAX sample transpose exceeds XM's signed-byte range",
                            );
                        }
                        let native_pitch = (i32::from(row.relative_note) - 2
                            + if row.fixed_note {
                                0
                            } else {
                                i32::from(note) - 2 + i32::from(order.transpose)
                            })
                            * 32
                            + i32::from(setting.pitch);
                        if !(0..=0xEF1).contains(&native_pitch) {
                            warning(
                                &mut warnings,
                                setting.source.effective_offset as usize,
                                "GAX pitch reaches an engine clamp that XM cannot preserve",
                            );
                        }
                    }
                    let note = if row.fixed_note {
                        i16::from(row.relative_note)
                    } else {
                        i16::from(note) + i16::from(order.transpose)
                    };
                    if !(1..=96).contains(&note) {
                        warning(
                            &mut warnings,
                            channel.order_table.effective_offset as usize,
                            "GAX transposed note is outside XM's note range",
                        );
                    }
                }
            }
        }
    }
    warnings.sort();
    warnings.dedup();
    spans.sort();
    spans.dedup();
    Ok(GaxSong { header: RomSpan::new(at, HEADER_LEN), version: version.into(), version_span, title, artist, metadata,
        rows_per_pattern: rows as u16, restart_order: restart, volume, sample_rate, channels, patterns, instruments, samples,
        mapped_spans: spans, xm_exportable: warnings.is_empty(), warnings,
        projection_limitations: vec!["XM approximates the GAX mixer, envelope levels, and frame timing (149 BPM); it is not native engine playback".into(),
            "Sample loops use the independent replayer's end-minus-start period with an exclusive end; GBA playback has not been compared on a retail corpus".into()] })
}

fn read_pattern(
    bytes: &[u8],
    at: usize,
    row_count: usize,
    budget: &mut Budget<'_>,
    warnings: &mut Vec<GaxWarning>,
) -> ReadResult<GaxPattern> {
    let empty = *need(bytes.get(at))?;
    check(empty <= 1)?;
    let mut rows = Vec::with_capacity(row_count);
    let mut cursor = at + 1;
    if empty == 1 {
        rows.resize(row_count, GaxCell::default());
    }
    while rows.len() < row_count {
        budget.charge()?;
        let command_at = cursor;
        let flag = *need(bytes.get(cursor))?;
        cursor += 1;
        let mut cell = GaxCell::default();
        match flag {
            0x80 => (),
            0xFF => {
                let len = usize::from(*need(bytes.get(cursor))?);
                cursor += 1;
                check(len > 0 && rows.len() + len <= row_count)?;
                rows.resize(rows.len() + len, cell);
                continue;
            }
            0xFA..=0xFE => {
                cell.effect = *need(bytes.get(cursor))?;
                cell.parameter = *need(bytes.get(cursor + 1))?;
                cursor += 2;
            }
            _ => {
                cell.note = Some(flag & 0x7F);
                cell.instrument = Some(*need(bytes.get(cursor))?);
                cursor += 1;
                if flag & 0x80 == 0 {
                    cell.effect = *need(bytes.get(cursor))?;
                    cell.parameter = *need(bytes.get(cursor + 1))?;
                    cursor += 2;
                }
            }
        }
        match cell.effect {
            0 if cell.parameter == 0 => (),
            12 => (),
            15 if (1..=31).contains(&cell.parameter) => (),
            14 if cell.parameter >> 4 == 13 => (),
            _ => warning(
                warnings,
                command_at,
                "GAX pattern effect cannot yet be preserved in XM",
            ),
        }
        if cell.note == Some(1) && cell.instrument.is_some_and(|i| i != 0) {
            warning(
                warnings,
                command_at,
                "GAX combined note-off and instrument change needs runtime handling",
            );
        }
        if cell.note.is_some_and(|note| note > 1) && cell.instrument == Some(0) {
            warning(
                warnings,
                command_at,
                "GAX pitch change retains sample phase and envelope; an XM note would retrigger",
            );
        }
        if cell.note == Some(0) && cell.instrument.is_some_and(|index| index != 0) {
            warning(
                warnings,
                command_at,
                "GAX instrument-only retrigger cannot yet be preserved in XM",
            );
        }
        rows.push(cell);
    }
    Ok(GaxPattern {
        source: RomSpan::new(at, cursor - at),
        rows,
    })
}

fn read_title(
    bytes: &[u8],
    at: usize,
    end: usize,
    budget: &mut Budget<'_>,
) -> ReadResult<(String, String, RomSpan)> {
    check(end > at && end - at <= 256)?;
    let mut raw = Vec::new();
    for pos in at..end {
        budget.charge()?;
        raw.push(*need(bytes.get(pos))?);
    }
    let padding = raw.iter().rev().take_while(|b| **b == 0).count();
    check(padding <= 3)?;
    raw.truncate(raw.len() - padding);
    check(raw.first() == Some(&b'"'))?;
    let close = need(
        raw.iter()
            .enumerate()
            .skip(1)
            .find(|(_, b)| **b == b'"')
            .map(|(i, _)| i),
    )?;
    check(raw.get(close + 1..close + 4) == Some(&[b' ', 0xA9, b' '][..]))?;
    check(raw.len() > close + 4 && raw.iter().all(|b| *b >= 32 && *b != 127))?;
    let latin1 = |b: &[u8]| b.iter().map(|v| char::from(*v)).collect::<String>();
    Ok((
        latin1(&raw[1..close]),
        latin1(&raw[close + 4..]),
        RomSpan::new(at, end - at),
    ))
}

pub mod project;

pub use project::project;

#[cfg(test)]
#[path = "gax/tests.rs"]
mod tests;
