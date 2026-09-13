use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop, tracker};
use bounds::{GraphBudget, retained_owned_bytes};

mod bounds;
mod flow;

const BANK_ID: u16 = 0x0121;
const INSTRUMENT_HEADER_LEN: usize = 124;
const MAX_PATTERN_ROWS: usize = 4096;
const MAX_SONG_CELLS: usize = 262_144;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EngineSoftwareSong {
    pub header: RomSpan,
    pub bank: RomSpan,
    pub index: u16,
    pub title: String,
    pub channels: u16,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
enum ReadError {
    Invalid,
    Stop(ScanStop),
}

type ReadResult<T> = std::result::Result<T, ReadError>;

trait Meter {
    fn charge(&mut self) -> std::result::Result<(), ScanStop>;
}

impl Meter for Budget<'_> {
    fn charge(&mut self) -> std::result::Result<(), ScanStop> {
        self.charge()
    }
}

struct ExportMeter<'a> {
    cancel: &'a AtomicBool,
    remaining: u64,
}

impl Meter for ExportMeter<'_> {
    fn charge(&mut self) -> std::result::Result<(), ScanStop> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(ScanStop::Cancelled);
        }
        if self.remaining == 0 {
            return Err(ScanStop::WorkLimit);
        }
        self.remaining -= 1;
        Ok(())
    }
}

#[derive(Clone)]
struct Bank {
    span: RomSpan,
    instruments: Vec<Instrument>,
    songs: Vec<SongGraph>,
    common_spans: Vec<RomSpan>,
    warnings: Vec<String>,
}

#[derive(Clone)]
struct Instrument {
    sample: RomSpan,
    loop_range: Option<(u32, u32)>,
    volume: u8,
    panning: u8,
    finetune: i8,
    relative_note: i8,
    fadeout: u16,
    volume_envelope: Envelope,
    panning_envelope: Envelope,
}

#[derive(Clone)]
struct Envelope {
    span: RomSpan,
    points: Vec<(u16, u16)>,
    sustain: Option<u8>,
    loop_range: Option<(u8, u8)>,
    xm_valid: bool,
}

#[derive(Clone)]
struct SongGraph {
    index: u16,
    header: RomSpan,
    table_entry: RomSpan,
    channels: u8,
    restart: u8,
    speed: u8,
    tempo: u8,
    orders: Vec<u8>,
    patterns: Vec<Pattern>,
    spans: Vec<RomSpan>,
    warnings: Vec<String>,
}

#[derive(Clone)]
struct Pattern {
    cells: Vec<Cell>,
}

#[derive(Clone, Copy, Default)]
struct Cell {
    note: u8,
    instrument: u8,
    volume: u8,
    effect: u8,
    parameter: u8,
}

fn checked<M: Meter>(meter: &mut M, valid: bool) -> ReadResult<()> {
    meter.charge().map_err(ReadError::Stop)?;
    valid.then_some(()).ok_or(ReadError::Invalid)
}

fn bytes_at(bytes: &[u8], at: usize, len: usize) -> ReadResult<&[u8]> {
    bytes
        .get(at..at.checked_add(len).ok_or(ReadError::Invalid)?)
        .ok_or(ReadError::Invalid)
}

fn byte_at(bytes: &[u8], at: usize) -> ReadResult<u8> {
    bytes.get(at).copied().ok_or(ReadError::Invalid)
}

fn u16_at(bytes: &[u8], at: usize) -> ReadResult<u16> {
    let bytes = bytes_at(bytes, at, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> ReadResult<u32> {
    let bytes = bytes_at(bytes, at, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn align4(value: usize) -> ReadResult<usize> {
    value
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or(ReadError::Invalid)
}

fn span(at: usize, len: usize) -> ReadResult<RomSpan> {
    (at <= crate::MAX_ROM_BYTES && len <= crate::MAX_ROM_BYTES - at)
        .then(|| RomSpan::new(at, len))
        .ok_or(ReadError::Invalid)
}

fn add_warning(warnings: &mut Vec<String>, text: &str) {
    if !warnings.iter().any(|warning| warning == text) {
        warnings.push(text.into());
    }
}

/// Finds complete 0x0121 banks, retaining zero row-data pointers as empty rows.
pub(crate) fn scan(
    bytes: &[u8],
    output: &mut Vec<EngineSoftwareSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> Result<(), ScanStop> {
    scan_with_inventory_limit(bytes, output, budget, max_candidates, MAX_RETAINED_BYTES)
}

fn scan_with_inventory_limit(
    bytes: &[u8],
    output: &mut Vec<EngineSoftwareSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    retained_limit: usize,
) -> Result<(), ScanStop> {
    if output.len() >= max_candidates {
        return Err(ScanStop::CandidateLimit);
    }
    let mut retained = retained_owned_bytes(output);
    if retained > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    let mut seen_banks = BTreeSet::new();
    for at in (0..=bytes.len().saturating_sub(4)).step_by(4) {
        if at.is_multiple_of(64) {
            budget.charge()?;
        }
        if bytes.get(at..at + 2) != Some(&BANK_ID.to_le_bytes()) || !seen_banks.insert(at) {
            continue;
        }
        let bank = match parse_bank(bytes, at, budget) {
            Ok(bank) => bank,
            Err(ReadError::Invalid) => continue,
            Err(ReadError::Stop(stop)) => return Err(stop),
        };
        for song in bank.songs.iter().map(|song| public_song(&bank, song)) {
            if output.len() >= max_candidates {
                return Err(ScanStop::CandidateLimit);
            }
            bounds::push_song(output, song, &mut retained, retained_limit)?;
        }
    }
    Ok(())
}

fn parse_bank<M: Meter>(bytes: &[u8], base: usize, meter: &mut M) -> ReadResult<Bank> {
    let mut graph_budget = GraphBudget::new(MAX_RETAINED_BYTES);
    checked(
        meter,
        base.is_multiple_of(4) && u16_at(bytes, base)? == BANK_ID,
    )?;
    let instrument_count = usize::from(byte_at(bytes, base + 2)?);
    let song_count = usize::from(byte_at(bytes, base + 3)?);
    checked(meter, instrument_count > 0 && song_count > 0)?;
    let table_len = song_count.checked_mul(4).ok_or(ReadError::Invalid)?;
    let header_len = 4usize.checked_add(table_len).ok_or(ReadError::Invalid)?;
    bytes_at(bytes, base, header_len)?;
    let header_span = span(base, header_len)?;
    let mut common_spans = Vec::new();
    graph_budget.reserve(&mut common_spans, 1 + instrument_count * 4)?;
    common_spans.push(header_span);
    let mut warnings = Vec::new();
    let mut cursor = base.checked_add(header_len).ok_or(ReadError::Invalid)?;
    let mut instruments = Vec::new();
    graph_budget.reserve(&mut instruments, instrument_count)?;
    for _ in 0..instrument_count {
        meter.charge().map_err(ReadError::Stop)?;
        bytes_at(bytes, cursor, INSTRUMENT_HEADER_LEN)?;
        let sample_len = u32_at(bytes, cursor)? as usize;
        let sample_at = cursor
            .checked_add(INSTRUMENT_HEADER_LEN)
            .ok_or(ReadError::Invalid)?;
        bytes_at(bytes, sample_at, sample_len)?;
        let next = align4(
            sample_at
                .checked_add(sample_len)
                .ok_or(ReadError::Invalid)?,
        )?;
        bytes_at(
            bytes,
            sample_at,
            next.checked_sub(sample_at).ok_or(ReadError::Invalid)?,
        )?;
        let sample_loop_start = u32_at(bytes, cursor + 4)?;
        let sample_loop_len = u32_at(bytes, cursor + 8)?;
        let loop_end = sample_loop_start.checked_add(sample_loop_len);
        let loop_range = match loop_end {
            Some(end) if end <= sample_len as u32 && sample_loop_len > 0 => {
                Some((sample_loop_start, end))
            }
            Some(_) | None if sample_loop_start != 0 || sample_loop_len != 0 => {
                add_warning(
                    &mut warnings,
                    "Engine Software sample loop is outside PCM; XM omits the loop",
                );
                None
            }
            _ => None,
        };
        let volume = byte_at(bytes, cursor + 12)?;
        if volume > 64 {
            add_warning(
                &mut warnings,
                "Engine Software sample volume is outside XM's supported range",
            );
        }
        let volume_envelope = parse_envelope(bytes, cursor + 20, meter, &mut graph_budget)?;
        let panning_envelope = parse_envelope(bytes, cursor + 72, meter, &mut graph_budget)?;
        if !volume_envelope.xm_valid || !panning_envelope.xm_valid {
            add_warning(
                &mut warnings,
                "Engine Software envelope metadata is invalid; XM omits that envelope",
            );
        }
        let header = span(cursor, INSTRUMENT_HEADER_LEN)?;
        let sample = span(sample_at, sample_len)?;
        common_spans.extend([header, sample, volume_envelope.span, panning_envelope.span]);
        instruments.push(Instrument {
            sample,
            loop_range,
            volume,
            panning: byte_at(bytes, cursor + 13)?,
            finetune: byte_at(bytes, cursor + 14)? as i8,
            relative_note: byte_at(bytes, cursor + 15)? as i8,
            fadeout: u16_at(bytes, cursor + 16)?,
            volume_envelope,
            panning_envelope,
        });
        cursor = next;
    }
    let bank_span = span(base, cursor.checked_sub(base).ok_or(ReadError::Invalid)?)?;
    let mut songs = Vec::new();
    graph_budget.reserve(&mut songs, song_count)?;
    for index in 0..song_count {
        meter.charge().map_err(ReadError::Stop)?;
        let table_at = base + 4 + index * 4;
        let offset = u32_at(bytes, table_at)? as usize;
        let song_at = base.checked_add(offset).ok_or(ReadError::Invalid)?;
        checked(meter, song_at >= cursor)?;
        let mut song = parse_song(
            bytes,
            base,
            SongEntry {
                at: song_at,
                index: index as u16,
                table_entry: span(table_at, 4)?,
            },
            instruments.len(),
            meter,
            &mut graph_budget,
        )?;
        for warning in &warnings {
            add_warning(&mut song.warnings, warning);
        }
        songs.push(song);
    }
    common_spans.sort();
    common_spans.dedup();
    Ok(Bank {
        span: bank_span,
        instruments,
        songs,
        common_spans,
        warnings,
    })
}

fn parse_envelope<M: Meter>(
    bytes: &[u8],
    at: usize,
    meter: &mut M,
    graph_budget: &mut GraphBudget,
) -> ReadResult<Envelope> {
    bytes_at(bytes, at, 52)?;
    let count = usize::from(byte_at(bytes, at)?);
    checked(meter, count <= 12)?;
    let optional_index = |value: u8| -> Option<u8> { (value != u8::MAX).then_some(value) };
    let sustain = optional_index(byte_at(bytes, at + 1)?);
    let loop_start = optional_index(byte_at(bytes, at + 2)?);
    let loop_end = optional_index(byte_at(bytes, at + 3)?);
    let mut points = Vec::new();
    graph_budget.reserve(&mut points, count)?;
    let mut xm_valid = sustain.is_none_or(|value| usize::from(value) < count)
        && loop_start.is_some() == loop_end.is_some();
    for index in 0..count {
        meter.charge().map_err(ReadError::Stop)?;
        let point_at = at + 4 + index * 4;
        let point = (u16_at(bytes, point_at)?, u16_at(bytes, point_at + 2)?);
        xm_valid &= point.1 <= 64
            && points
                .last()
                .is_none_or(|last: &(u16, u16)| last.0 < point.0);
        points.push(point);
    }
    let loop_range = loop_start.zip(loop_end);
    if let Some((start, end)) = loop_range {
        xm_valid &= start <= end && usize::from(end) < count;
    }
    Ok(Envelope {
        span: span(at, 52)?,
        points,
        sustain,
        loop_range,
        xm_valid,
    })
}

struct SongEntry {
    at: usize,
    index: u16,
    table_entry: RomSpan,
}

fn parse_song<M: Meter>(
    bytes: &[u8],
    base: usize,
    entry: SongEntry,
    instruments: usize,
    meter: &mut M,
    graph_budget: &mut GraphBudget,
) -> ReadResult<SongGraph> {
    let SongEntry {
        at,
        index,
        table_entry,
    } = entry;
    bytes_at(bytes, at, 8)?;
    let channels = usize::from(byte_at(bytes, at)?);
    let order_count = usize::from(byte_at(bytes, at + 1)?);
    let restart = usize::from(byte_at(bytes, at + 2)?);
    let pattern_count = usize::from(byte_at(bytes, at + 3)?);
    let speed = byte_at(bytes, at + 4)?;
    let tempo = byte_at(bytes, at + 5)?;
    checked(
        meter,
        (1..=32).contains(&channels)
            && (1..=255).contains(&order_count)
            && restart < order_count
            && (1..=255).contains(&pattern_count)
            && (1..=31).contains(&usize::from(speed))
            && (32..=255).contains(&usize::from(tempo)),
    )?;
    let orders_at = at.checked_add(8).ok_or(ReadError::Invalid)?;
    let mut orders = Vec::new();
    graph_budget.reserve(&mut orders, order_count)?;
    orders.extend_from_slice(bytes_at(bytes, orders_at, order_count)?);
    checked(
        meter,
        orders
            .iter()
            .all(|order| usize::from(*order) < pattern_count),
    )?;
    let mut spans = Vec::new();
    graph_budget.reserve(&mut spans, 2)?;
    spans.extend([span(at, 8)?, span(orders_at, order_count)?]);
    let mut warnings = Vec::new();
    let mut cursor = align4(
        orders_at
            .checked_add(order_count)
            .ok_or(ReadError::Invalid)?,
    )?;
    let mut total_cells = 0usize;
    let mut patterns = Vec::new();
    graph_budget.reserve(&mut patterns, pattern_count)?;
    for _ in 0..pattern_count {
        meter.charge().map_err(ReadError::Stop)?;
        let pattern_at = cursor;
        let row_count = usize::from(u16_at(bytes, pattern_at)?);
        checked(meter, row_count <= MAX_PATTERN_ROWS)?;
        total_cells = total_cells
            .checked_add(row_count.checked_mul(channels).ok_or(ReadError::Invalid)?)
            .ok_or(ReadError::Invalid)?;
        checked(meter, total_cells <= MAX_SONG_CELLS)?;
        let rows_at = align4(pattern_at.checked_add(2).ok_or(ReadError::Invalid)?)?;
        let table_len = row_count.checked_mul(4).ok_or(ReadError::Invalid)?;
        bytes_at(bytes, rows_at, table_len)?;
        graph_budget.reserve(&mut spans, row_count + 2)?;
        spans.extend([span(pattern_at, 4)?, span(rows_at, table_len)?]);
        let mut cells = Vec::new();
        graph_budget.reserve(&mut cells, row_count * channels)?;
        for row in 0..row_count {
            meter.charge().map_err(ReadError::Stop)?;
            let row_pointer = u32_at(bytes, rows_at + row * 4)? as usize;
            if row_pointer == 0 {
                cells.resize(cells.len() + channels, Cell::default());
                continue;
            }
            let row_at = base.checked_add(row_pointer).ok_or(ReadError::Invalid)?;
            let (row_cells, row_span) = parse_row(bytes, row_at, channels, instruments, meter)?;
            cells.extend(row_cells);
            spans.push(row_span);
        }
        if row_count > 256 {
            add_warning(
                &mut warnings,
                "Engine Software pattern exceeds 256 rows; XM splits it into sequential patterns",
            );
        }
        patterns.push(Pattern { cells });
        cursor = rows_at.checked_add(table_len).ok_or(ReadError::Invalid)?;
    }
    if patterns.iter().any(|pattern| pattern.cells.is_empty()) {
        flow::validate_zero_patterns(channels, &orders, restart, &patterns, meter)?;
        add_warning(
            &mut warnings,
            "Engine Software zero-row patterns are unreachable; XM omits them and remaps order jumps",
        );
    }
    spans.sort();
    spans.dedup();
    Ok(SongGraph {
        index,
        header: span(at, 8)?,
        table_entry,
        channels: channels as u8,
        restart: restart as u8,
        speed,
        tempo,
        orders,
        patterns,
        spans,
        warnings,
    })
}

fn parse_row<M: Meter>(
    bytes: &[u8],
    at: usize,
    channels: usize,
    instruments: usize,
    meter: &mut M,
) -> ReadResult<(Vec<Cell>, RomSpan)> {
    let mask_len = (channels * 5).div_ceil(8);
    let mask = bytes_at(bytes, at, mask_len)?;
    let mut values = mask_len;
    for bit in 0..channels * 5 {
        if mask[bit / 8] & (0x80 >> (bit % 8)) != 0 {
            values = values.checked_add(1).ok_or(ReadError::Invalid)?;
        }
    }
    bytes_at(bytes, at, values)?;
    let mut next = at + mask_len;
    let mut cells = Vec::with_capacity(channels);
    for channel in 0..channels {
        meter.charge().map_err(ReadError::Stop)?;
        let mut cell = Cell::default();
        for (column, field) in [
            &mut cell.note,
            &mut cell.instrument,
            &mut cell.volume,
            &mut cell.effect,
            &mut cell.parameter,
        ]
        .into_iter()
        .enumerate()
        {
            let bit = channel * 5 + column;
            if mask[bit / 8] & (0x80 >> (bit % 8)) != 0 {
                *field = byte_at(bytes, next)?;
                next += 1;
            }
        }
        checked(
            meter,
            cell.note <= 97 && usize::from(cell.instrument) <= instruments,
        )?;
        cells.push(cell);
    }
    Ok((cells, span(at, values)?))
}

fn public_song(bank: &Bank, song: &SongGraph) -> EngineSoftwareSong {
    let mut mapped_spans = bank.common_spans.clone();
    mapped_spans.extend(song.spans.iter().copied());
    mapped_spans.push(song.table_entry);
    mapped_spans.sort();
    mapped_spans.dedup();
    let mut warnings = bank.warnings.clone();
    for warning in &song.warnings {
        add_warning(&mut warnings, warning);
    }
    warnings.sort();
    EngineSoftwareSong {
        header: song.header,
        bank: bank.span,
        index: song.index,
        title: format!(
            "Engine Software {:08X} song {:02}",
            bank.span.canonical_cpu_address, song.index
        ),
        channels: u16::from(song.channels),
        mapped_spans,
        warnings,
    }
}

pub fn to_xm(bytes: &[u8], song: &EngineSoftwareSong, cancel: &AtomicBool) -> Result<Vec<u8>> {
    ensure!(
        song.bank.canonical_cpu_address == 0x0800_0000 + song.bank.effective_offset,
        "Engine Software bank has an invalid canonical address"
    );
    let mut meter = ExportMeter {
        cancel,
        remaining: crate::MAX_SCAN_WORK,
    };
    let bank =
        parse_bank(bytes, song.bank.effective_offset as usize, &mut meter).map_err(|error| {
            match error {
                ReadError::Invalid => {
                    anyhow::anyhow!("Engine Software source graph is no longer valid")
                }
                ReadError::Stop(ScanStop::Cancelled) => {
                    anyhow::anyhow!("Engine Software export cancelled")
                }
                ReadError::Stop(stop) => {
                    anyhow::anyhow!("Engine Software export stopped: {stop:?}")
                }
            }
        })?;
    let actual = bank
        .songs
        .get(usize::from(song.index))
        .map(|graph| public_song(&bank, graph))
        .context("Engine Software song index is outside its validated bank")?;
    ensure!(
        actual == *song,
        "Engine Software song does not match its validated inventory"
    );
    let graph = bank
        .songs
        .get(usize::from(song.index))
        .context("Engine Software song graph is absent")?;
    let module = project::project_xm(bytes, &bank, graph, song, cancel)?;
    tracker::xm::encode(&module, cancel)
}

mod project;

#[cfg(test)]
#[path = "engine_software/tests.rs"]
mod tests;
