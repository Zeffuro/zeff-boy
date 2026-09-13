use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};

use super::{Bank, EngineSoftwareSong, Envelope, SongGraph, bytes_at};
use crate::tracker;

pub(super) fn project_xm(
    bytes: &[u8],
    bank: &Bank,
    song: &SongGraph,
    inventory: &EngineSoftwareSong,
    cancel: &AtomicBool,
) -> Result<tracker::xm::Module> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Engine Software export cancelled"
    );
    let mut source_instruments = BTreeSet::new();
    for pattern in &song.patterns {
        for cell in &pattern.cells {
            if cell.instrument != 0 {
                source_instruments.insert(cell.instrument);
            }
            ensure!(
                valid_xm_volume(cell.volume),
                "Engine Software volume column cannot be represented exactly in XM"
            );
        }
    }
    ensure!(
        source_instruments.len() <= 128,
        "Engine Software song uses more than XM's 128 instruments"
    );
    let instrument_map: BTreeMap<_, _> = source_instruments
        .iter()
        .enumerate()
        .map(|(projected, source)| (*source, (projected + 1) as u8))
        .collect();
    let remap_jumps = song.patterns.iter().any(|pattern| pattern.cells.is_empty());
    let mut order_map = [0; 256];
    let mut next_order = 0;
    if remap_jumps {
        for (index, pattern) in song.orders.iter().enumerate() {
            order_map[index] = next_order;
            if !song.patterns[usize::from(*pattern)].cells.is_empty() {
                next_order += 1;
            }
        }
    }
    let mut patterns = Vec::new();
    let mut pattern_chunks = Vec::with_capacity(song.patterns.len());
    for pattern in &song.patterns {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "Engine Software export cancelled"
        );
        let mut chunks = Vec::new();
        for cells in pattern.cells.chunks(usize::from(song.channels) * 256) {
            let rows = cells.len() / usize::from(song.channels);
            ensure!(
                (1..=256).contains(&rows),
                "Engine Software XM pattern has invalid rows"
            );
            ensure!(
                patterns.len() < 256,
                "Engine Software song exceeds XM's pattern limit"
            );
            let index = patterns.len() as u8;
            let projected = cells
                .iter()
                .map(|cell| tracker::xm::Cell {
                    note: cell.note,
                    instrument: cell
                        .instrument
                        .checked_sub(1)
                        .and_then(|source| instrument_map.get(&(source + 1)).copied())
                        .unwrap_or_default(),
                    volume: cell.volume,
                    effect: cell.effect,
                    parameter: if remap_jumps && cell.effect == 0x0b {
                        order_map[usize::from(cell.parameter)]
                    } else {
                        cell.parameter
                    },
                })
                .collect();
            patterns.push(tracker::xm::Pattern {
                rows: rows as u16,
                cells: projected,
            });
            chunks.push(index);
        }
        pattern_chunks.push(chunks);
    }
    let mut orders = Vec::new();
    let mut restart = None;
    for (order_index, source_pattern) in song.orders.iter().enumerate() {
        if order_index == usize::from(song.restart) {
            restart = Some(orders.len() as u16);
        }
        orders.extend_from_slice(&pattern_chunks[usize::from(*source_pattern)]);
    }
    ensure!(
        !orders.is_empty() && orders.len() <= 256,
        "Engine Software song expands beyond XM's order limit"
    );
    let mut instruments = Vec::with_capacity(source_instruments.len());
    for source in source_instruments {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "Engine Software export cancelled"
        );
        let instrument = bank
            .instruments
            .get(usize::from(source - 1))
            .context("Engine Software cell references a missing instrument")?;
        ensure!(
            instrument.volume <= 64,
            "Engine Software sample volume is outside XM's supported range"
        );
        let mut projected = tracker::xm::Instrument {
            name: format!("Engine Software instrument {source:02}"),
            ..Default::default()
        };
        projected.volume_envelope = xm_envelope(&instrument.volume_envelope);
        projected.panning_envelope = xm_envelope(&instrument.panning_envelope);
        projected.fadeout = instrument.fadeout;
        if instrument.sample.byte_len != 0 {
            let start = instrument.sample.effective_offset as usize;
            let end = start + instrument.sample.byte_len as usize;
            let pcm = bytes_at(bytes, start, end - start)
                .map_err(|_| anyhow::anyhow!("Engine Software sample span exceeds source"))?
                .iter()
                .map(|sample| i16::from(*sample as i8) * 256)
                .collect();
            projected.samples.push(tracker::xm::Sample {
                name: format!("Engine Software sample {source:02}"),
                pcm,
                sixteen_bit: false,
                loop_range: instrument.loop_range,
                ping_pong: false,
                volume: instrument.volume,
                panning: instrument.panning,
                relative_note: instrument.relative_note,
                finetune: instrument.finetune,
            });
        }
        instruments.push(projected);
    }
    Ok(tracker::xm::Module {
        name: inventory.title.clone(),
        channels: u16::from(song.channels),
        orders,
        restart: restart.context("Engine Software song restart is absent")?,
        speed: u16::from(song.speed),
        bpm: u16::from(song.tempo),
        linear_frequency: false,
        patterns,
        instruments,
    })
}

fn xm_envelope(envelope: &Envelope) -> tracker::xm::Envelope {
    if envelope.xm_valid {
        tracker::xm::Envelope {
            points: envelope.points.clone(),
            sustain: envelope.sustain,
            loop_range: envelope.loop_range,
        }
    } else {
        tracker::xm::Envelope::default()
    }
}

fn valid_xm_volume(value: u8) -> bool {
    value == 0 || (0x10..=0x50).contains(&value) || value >= 0x60
}
