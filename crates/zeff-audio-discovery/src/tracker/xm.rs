use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};

pub(crate) const MAX_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const MAX_CELLS: usize = 2 * 1024 * 1024;

pub struct Module {
    pub name: String,
    pub channels: u16,
    pub orders: Vec<u8>,
    pub restart: u16,
    pub speed: u16,
    pub bpm: u16,
    pub linear_frequency: bool,
    pub patterns: Vec<Pattern>,
    pub instruments: Vec<Instrument>,
}

pub struct Pattern {
    pub rows: u16,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub note: u8,
    pub instrument: u8,
    pub volume: u8,
    pub effect: u8,
    pub parameter: u8,
}

pub struct Instrument {
    pub name: String,
    pub sample_for_key: [u8; 96],
    pub volume_envelope: Envelope,
    pub panning_envelope: Envelope,
    pub fadeout: u16,
    pub vibrato: [u8; 4],
    pub samples: Vec<Sample>,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            name: String::new(),
            sample_for_key: [0; 96],
            volume_envelope: Envelope::default(),
            panning_envelope: Envelope::default(),
            fadeout: 0,
            vibrato: [0; 4],
            samples: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct Envelope {
    pub points: Vec<(u16, u16)>,
    pub sustain: Option<u8>,
    pub loop_range: Option<(u8, u8)>,
}

pub struct Sample {
    pub name: String,
    pub pcm: Vec<i16>,
    pub sixteen_bit: bool,
    pub loop_range: Option<(u32, u32)>,
    pub ping_pong: bool,
    pub volume: u8,
    pub panning: u8,
    pub relative_note: i8,
    pub finetune: i8,
}

pub fn encode(module: &Module, cancel: &AtomicBool) -> Result<Vec<u8>> {
    validate(module, cancel)?;
    let mut out = Vec::new();
    out.extend_from_slice(b"Extended Module: ");
    name(&mut out, &module.name, 20);
    out.push(0x1a);
    name(&mut out, "Zeff-boy", 20);
    u16le(&mut out, 0x0104);
    u32le(&mut out, 276);
    for value in [
        module.orders.len() as u16,
        module.restart,
        module.channels,
        module.patterns.len() as u16,
        module.instruments.len() as u16,
        u16::from(module.linear_frequency),
        module.speed,
        module.bpm,
    ] {
        u16le(&mut out, value);
    }
    out.extend_from_slice(&module.orders);
    out.resize(336, 0);
    for pattern in &module.patterns {
        cancelled(cancel)?;
        let mut data = Vec::new();
        for cell in &pattern.cells {
            let values = [
                cell.note,
                cell.instrument,
                cell.volume,
                cell.effect,
                cell.parameter,
            ];
            let mask = values.iter().enumerate().fold(0x80, |mask, (bit, value)| {
                mask | (u8::from(*value != 0) << bit)
            });
            if mask == 0x9f {
                data.extend_from_slice(&values);
            } else {
                data.push(mask);
                for (bit, value) in values.iter().enumerate() {
                    if mask & (1 << bit) != 0 {
                        data.push(*value);
                    }
                }
            }
        }
        ensure!(
            data.len() <= u16::MAX as usize,
            "XM pattern exceeds 65535 packed bytes"
        );
        u32le(&mut out, 9);
        out.push(0);
        u16le(&mut out, pattern.rows);
        u16le(&mut out, data.len() as u16);
        out.extend_from_slice(&data);
    }
    for instrument in &module.instruments {
        cancelled(cancel)?;
        let start = out.len();
        u32le(
            &mut out,
            if instrument.samples.is_empty() {
                29
            } else {
                263
            },
        );
        name(&mut out, &instrument.name, 22);
        out.push(0);
        u16le(&mut out, instrument.samples.len() as u16);
        if instrument.samples.is_empty() {
            continue;
        }
        u32le(&mut out, 40);
        out.extend_from_slice(&instrument.sample_for_key);
        for envelope in [&instrument.volume_envelope, &instrument.panning_envelope] {
            for index in 0..12 {
                let (x, y) = envelope.points.get(index).copied().unwrap_or_default();
                u16le(&mut out, x);
                u16le(&mut out, y);
            }
        }
        out.push(instrument.volume_envelope.points.len() as u8);
        out.push(instrument.panning_envelope.points.len() as u8);
        for envelope in [&instrument.volume_envelope, &instrument.panning_envelope] {
            out.push(envelope.sustain.unwrap_or_default());
            let (a, b) = envelope.loop_range.unwrap_or_default();
            out.extend_from_slice(&[a, b]);
        }
        for envelope in [&instrument.volume_envelope, &instrument.panning_envelope] {
            out.push(
                u8::from(!envelope.points.is_empty())
                    | (u8::from(envelope.sustain.is_some()) << 1)
                    | (u8::from(envelope.loop_range.is_some()) << 2),
            );
        }
        out.extend_from_slice(&instrument.vibrato);
        u16le(&mut out, instrument.fadeout);
        out.resize(start + 263, 0);
        for sample in &instrument.samples {
            let stride = if sample.sixteen_bit { 2 } else { 1 };
            u32le(&mut out, (sample.pcm.len() * stride) as u32);
            let (a, b) = sample.loop_range.unwrap_or_default();
            u32le(&mut out, a * stride as u32);
            u32le(&mut out, (b - a) * stride as u32);
            out.extend_from_slice(&[
                sample.volume,
                sample.finetune as u8,
                if sample.loop_range.is_some() {
                    if sample.ping_pong { 2 } else { 1 }
                } else {
                    0
                } | if sample.sixteen_bit { 0x10 } else { 0 },
                sample.panning,
                sample.relative_note as u8,
                0,
            ]);
            name(&mut out, &sample.name, 22);
        }
        for sample in &instrument.samples {
            let mut previous = 0i16;
            for chunk in sample.pcm.chunks(4096) {
                cancelled(cancel)?;
                for &value in chunk {
                    if sample.sixteen_bit {
                        out.extend_from_slice(&value.wrapping_sub(previous).to_le_bytes());
                    } else {
                        out.push(((value >> 8) as i8).wrapping_sub((previous >> 8) as i8) as u8);
                    }
                    previous = value;
                }
            }
        }
        ensure!(
            out.len() <= MAX_BYTES,
            "XM exceeds the 128 MiB output limit"
        );
    }
    Ok(out)
}

fn validate(module: &Module, cancel: &AtomicBool) -> Result<()> {
    cancelled(cancel)?;
    ensure!(
        (1..=32).contains(&module.channels),
        "XM requires 1 to 32 channels"
    );
    ensure!(
        !module.orders.is_empty() && module.orders.len() <= 256,
        "XM requires 1 to 256 orders"
    );
    ensure!(
        !module.patterns.is_empty() && module.patterns.len() <= 256,
        "XM requires 1 to 256 patterns"
    );
    ensure!(
        module.instruments.len() <= 128,
        "XM supports at most 128 instruments"
    );
    ensure!(
        (module.restart as usize) < module.orders.len(),
        "XM restart is outside the order list"
    );
    ensure!(
        module
            .orders
            .iter()
            .all(|&value| (value as usize) < module.patterns.len()),
        "XM order references an absent pattern"
    );
    ensure!(
        (1..=31).contains(&module.speed) && (32..=255).contains(&module.bpm),
        "XM tempo is outside the supported range"
    );
    let mut cell_count = 0;
    for pattern in &module.patterns {
        cancelled(cancel)?;
        ensure!(
            (1..=256).contains(&pattern.rows),
            "XM pattern requires 1 to 256 rows"
        );
        ensure!(
            pattern.cells.len() == usize::from(pattern.rows) * usize::from(module.channels),
            "XM pattern dimensions do not match its cells"
        );
        cell_count += pattern.cells.len();
        ensure!(
            cell_count <= MAX_CELLS,
            "XM pattern cells exceed the inventory limit"
        );
        for cell in &pattern.cells {
            ensure!(cell.note <= 97, "XM note is outside 0..97");
            ensure!(
                usize::from(cell.instrument) <= module.instruments.len(),
                "XM cell references an absent instrument"
            );
            ensure!(
                cell.volume == 0 || (0x10..=0x50).contains(&cell.volume) || cell.volume >= 0x60,
                "XM volume column is invalid"
            );
        }
    }
    let mut sample_bytes = 0;
    for instrument in &module.instruments {
        cancelled(cancel)?;
        ensure!(
            instrument.samples.len() <= 16,
            "XM instrument exceeds 16 samples"
        );
        if !instrument.samples.is_empty() {
            ensure!(
                instrument
                    .sample_for_key
                    .iter()
                    .all(|&value| usize::from(value) < instrument.samples.len()),
                "XM key references an absent sample"
            );
        }
        for envelope in [&instrument.volume_envelope, &instrument.panning_envelope] {
            ensure!(envelope.points.len() <= 12, "XM envelope exceeds 12 points");
            ensure!(
                envelope.points.iter().all(|&(_, y)| y <= 64),
                "XM envelope level exceeds 64"
            );
            ensure!(
                envelope.points.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "XM envelope ticks must increase"
            );
            if let Some(index) = envelope.sustain {
                ensure!(
                    usize::from(index) < envelope.points.len(),
                    "XM sustain point is outside the envelope"
                );
            }
            if let Some((a, b)) = envelope.loop_range {
                ensure!(
                    a <= b && usize::from(b) < envelope.points.len(),
                    "XM envelope loop is invalid"
                );
            }
        }
        ensure!(instrument.vibrato[0] <= 3, "XM vibrato type is invalid");
        for sample in &instrument.samples {
            sample_bytes += sample.pcm.len() * if sample.sixteen_bit { 2 } else { 1 };
            ensure!(
                sample_bytes <= MAX_BYTES - 16 * 1024 * 1024,
                "XM samples exceed the output budget"
            );
            ensure!(sample.volume <= 64, "XM sample volume exceeds 64");
            if let Some((a, b)) = sample.loop_range {
                ensure!(
                    a < b && b as usize <= sample.pcm.len(),
                    "XM sample loop is outside PCM"
                );
            }
            if !sample.sixteen_bit {
                for chunk in sample.pcm.chunks(4096) {
                    cancelled(cancel)?;
                    ensure!(
                        chunk.iter().all(|value| *value & 255 == 0),
                        "Eight-bit XM output would discard sample precision"
                    );
                }
            }
        }
    }
    Ok(())
}

fn name(out: &mut Vec<u8>, value: &str, width: usize) {
    let start = out.len();
    out.extend(value.chars().take(width).map(|ch| {
        if ch.is_ascii() && !ch.is_ascii_control() {
            ch as u8
        } else {
            b'?'
        }
    }));
    out.resize(start + width, 0);
}

fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "tracker export cancelled");
    Ok(())
}
