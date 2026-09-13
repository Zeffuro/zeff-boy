use super::GaxSong;
use std::collections::BTreeMap;

pub fn project(
    bytes: &[u8],
    song: &GaxSong,
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<crate::tracker::xm::Module> {
    use crate::tracker::xm;
    use anyhow::{Context, ensure};
    use std::sync::atomic::Ordering;
    let cancelled = || -> anyhow::Result<()> {
        ensure!(!cancel.load(Ordering::Relaxed), "GAX projection cancelled");
        Ok(())
    };
    cancelled()?;
    ensure!(
        song.xm_exportable && song.warnings.is_empty(),
        "GAX XM projection unavailable: {}",
        song.warnings
            .first()
            .map_or("unsupported graph", |w| w.reason.as_str())
    );
    ensure!(
        !song.channels.is_empty() && song.channels.len() <= 32,
        "Invalid GAX channel count"
    );
    let orders = song.channels[0].orders.len();
    ensure!(
        (1..=255).contains(&orders) && song.channels.iter().all(|c| c.orders.len() == orders),
        "Invalid GAX order count"
    );
    ensure!(
        (1..=256).contains(&song.rows_per_pattern) && song.instruments.len() <= 128,
        "GAX graph exceeds XM dimensions"
    );
    let mut instruments = Vec::with_capacity(song.instruments.len());
    let mut instrument_map = BTreeMap::new();
    let mut projected_frames = 0usize;
    for instrument in &song.instruments {
        cancelled()?;
        ensure!(
            instrument.rows.len() == 1,
            "Runtime GAX instrument pattern is unsupported"
        );
        let row = &instrument.rows[0];
        let slot = usize::from(
            row.sample_slot
                .checked_sub(1)
                .context("GAX instrument has no static sample selection")?,
        );
        let setting = instrument
            .sample_settings
            .get(slot)
            .context("GAX sample setting is absent")?;
        let sample_index = *instrument
            .sample_indices
            .get(slot)
            .context("GAX sample slot exceeds the supported descriptor range")?;
        let source = song
            .samples
            .iter()
            .find(|s| s.index == sample_index)
            .context("GAX sample is absent")?;
        ensure!(
            setting.start_position == 0 && setting.modulation == 0,
            "GAX sample offset or modulation is unsupported"
        );
        ensure!(
            setting.loop_start <= setting.loop_end && setting.loop_end <= source.data.byte_len,
            "GAX sample loop bounds are invalid"
        );
        // Equal endpoints disable looping, including equal nonzero endpoints.
        let looped = setting.loop_start < setting.loop_end;
        let start = source.data.effective_offset as usize;
        let pcm_bytes = bytes
            .get(
                start
                    ..start
                        .checked_add(source.data.byte_len as usize)
                        .context("GAX sample range overflow")?,
            )
            .context("GAX sample is outside ROM")?;
        projected_frames = projected_frames
            .checked_add(pcm_bytes.len())
            .context("GAX projected sample size overflow")?;
        ensure!(
            projected_frames <= (xm::MAX_BYTES - 16 * 1024 * 1024) / 2,
            "GAX projected samples exceed the XM memory limit"
        );
        let mut pcm = Vec::with_capacity(pcm_bytes.len());
        for chunk in pcm_bytes.chunks(4096) {
            cancelled()?;
            pcm.extend(chunk.iter().map(|&b| (i16::from(b) - 128) * 256));
        }
        let pitch_semitones = setting.pitch / 32;
        let relative = pitch_semitones - 1
            + if row.fixed_note {
                0
            } else {
                i16::from(row.relative_note) - 2
            };
        let relative_note =
            i8::try_from(relative).context("GAX sample transpose exceeds XM range")?;
        let finetune = ((setting.pitch - pitch_semitones * 32) * 4) as i8;
        let mut level = 255u16;
        for [effect, parameter] in row.effects {
            if effect == 12 {
                level = u16::from(parameter);
            }
        }
        let envelope = &instrument.envelope;
        let volume_envelope = xm::Envelope {
            points: envelope
                .points
                .iter()
                .map(|p| {
                    (
                        p.tick,
                        ((u64::from(p.volume) * u64::from(level) * u64::from(song.volume) * 64
                            + 8_323_200)
                            / 16_646_400) as u16,
                    )
                })
                .collect(),
            sustain: envelope.sustain,
            loop_range: envelope.loop_start.zip(envelope.loop_end),
        };
        instruments.push(xm::Instrument {
            name: format!("GAX instrument {}", instrument.index),
            volume_envelope,
            fadeout: if envelope.sustain.is_some() {
                0
            } else {
                0xFFFF
            },
            samples: vec![xm::Sample {
                name: format!("GAX sample {}", source.index),
                pcm,
                sixteen_bit: false,
                loop_range: looped.then_some((setting.loop_start, setting.loop_end)),
                ping_pong: looped && setting.bidirectional,
                volume: 64,
                panning: 128,
                relative_note,
                finetune,
            }],
            ..xm::Instrument::default()
        });
        instrument_map.insert(instrument.index, instruments.len() as u8);
    }
    let mut patterns = Vec::with_capacity(orders);
    let mut active_instruments = vec![None; song.channels.len()];
    for order_index in 0..orders {
        let mut cells =
            Vec::with_capacity(usize::from(song.rows_per_pattern) * song.channels.len());
        for row_index in 0..usize::from(song.rows_per_pattern) {
            cancelled()?;
            for (channel_index, channel) in song.channels.iter().enumerate() {
                let order = channel.orders[order_index];
                let native = song
                    .patterns
                    .get(usize::from(order.pattern))
                    .and_then(|p| p.rows.get(row_index))
                    .context("GAX pattern cell is absent")?;
                let mut cell = xm::Cell::default();
                if let Some(note) = native.note {
                    if note == 1 {
                        cell.note = 97;
                    } else {
                        let native_index = native.instrument.context("GAX instrument is absent")?;
                        ensure!(
                            note == 0 || native_index != 0,
                            "GAX pitch change would lose retained sample phase in XM"
                        );
                        if native_index != 0 {
                            cell.instrument = *instrument_map
                                .get(&native_index)
                                .context("GAX instrument is unvalidated")?;
                            active_instruments[channel_index] = Some(cell.instrument);
                        }
                        if note != 0 {
                            let active = active_instruments[channel_index]
                                .context("GAX note has no initial instrument context")?;
                            let instrument = song
                                .instruments
                                .get(usize::from(active - 1))
                                .context("GAX active instrument is absent")?;
                            let instrument_row = instrument
                                .rows
                                .first()
                                .context("GAX active instrument has no static row")?;
                            let converted = if instrument_row.fixed_note {
                                i16::from(instrument_row.relative_note)
                            } else {
                                i16::from(note) + i16::from(order.transpose)
                            };
                            ensure!(
                                (1..=96).contains(&converted),
                                "GAX transposed note is outside XM range"
                            );
                            cell.note = converted as u8;
                        }
                    }
                }
                match native.effect {
                    0 if native.parameter == 0 => (),
                    12 => {
                        cell.effect = 12;
                        cell.parameter = ((u16::from(native.parameter) * 64 + 127) / 255) as u8;
                    }
                    15 if (1..=31).contains(&native.parameter) => {
                        cell.effect = 15;
                        cell.parameter = native.parameter;
                    }
                    14 if native.parameter >> 4 == 13 => {
                        cell.effect = 14;
                        cell.parameter = native.parameter;
                    }
                    _ => anyhow::bail!("GAX pattern effect cannot be preserved in XM"),
                }
                cells.push(cell);
            }
        }
        patterns.push(xm::Pattern {
            rows: song.rows_per_pattern,
            cells,
        });
    }
    Ok(xm::Module {
        name: song.title.clone(),
        channels: song.channels.len() as u16,
        orders: (0..orders as u8).collect(),
        restart: song.restart_order,
        speed: 6,
        bpm: 149,
        linear_frequency: true,
        patterns,
        instruments,
    })
}
