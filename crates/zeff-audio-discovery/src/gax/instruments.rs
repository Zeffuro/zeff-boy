use super::*;

pub(super) fn read_instrument(
    bytes: &[u8],
    table: usize,
    index: u8,
    budget: &mut Budget<'_>,
    warnings: &mut Vec<GaxWarning>,
) -> ReadResult<GaxInstrument> {
    let pointer = table + usize::from(index) * 4;
    let at = mapped(bytes, pointer, 24, 4)?;
    check(bytes[at] <= 1)?;
    let row_count = usize::from(bytes[at + 17]);
    check(row_count > 0)?;
    let rows_at = mapped(bytes, at + 20, row_count * 8, 4)?;
    let sample_indices: [u8; 4] = bytes[at + 1..at + 5].try_into().unwrap();
    let vibrato: [u8; 3] = bytes[at + 8..at + 11].try_into().unwrap();
    if bytes[at] != 0 {
        warning(warnings, at, "Blank GAX instrument has no projected voice");
    }
    if bytes[at + 16] == 0 {
        warning(
            warnings,
            at + 16,
            "GAX instrument performance list does not initialize a sample",
        );
    }
    let mut rows = Vec::with_capacity(row_count);
    let mut slots = 1usize;
    for row in 0..row_count {
        budget.charge()?;
        let p = rows_at + row * 8;
        check(bytes[p + 1] <= 1 && bytes[p + 2] <= 4)?;
        slots = slots.max(usize::from(bytes[p + 2]));
        rows.push(GaxInstrumentRow {
            relative_note: bytes[p],
            fixed_note: bytes[p + 1] != 0,
            sample_slot: bytes[p + 2],
            effects: [[bytes[p + 5], bytes[p + 4]], [bytes[p + 7], bytes[p + 6]]],
        });
        if bytes[p + 3] != 0 {
            warning(warnings, p + 3, "Unknown GAX instrument row flags");
        }
    }
    if row_count != 1 || rows[0].sample_slot == 0 || rows[0].relative_note == 0 {
        warning(
            warnings,
            rows_at,
            "Runtime GAX instrument pattern or unresolved sample selection",
        );
    }
    for row in &rows {
        for [effect, parameter] in row.effects {
            if effect != 12 && (effect != 0 || parameter != 0) {
                warning(
                    warnings,
                    rows_at,
                    "Runtime GAX instrument effect cannot be preserved in XM",
                );
            }
        }
    }
    if vibrato[1] != 0 {
        warning(
            warnings,
            at + 8,
            "GAX delayed vibrato differs from XM vibrato sweep",
        );
    }
    let mut sample_settings = Vec::with_capacity(slots);
    need(bytes.get(at..at + 24 + slots * 24))?;
    for slot in 0..slots {
        budget.charge()?;
        let p = at + 24 + slot * 24;
        check(bytes[p + 3] <= 1)?;
        let setting = GaxSampleSettings {
            source: RomSpan::new(p, 24),
            pitch: need(u16_at(bytes, p))? as i16,
            modulation: bytes[p + 2],
            bidirectional: bytes[p + 3] != 0,
            start_position: need(word(bytes, p + 4))? as i32,
            loop_start: need(word(bytes, p + 8))?,
            loop_end: need(word(bytes, p + 12))?,
            unknown_10: need(word(bytes, p + 16))? as i32,
            modulation_timer: need(u16_at(bytes, p + 20))?,
            unknown_16: need(u16_at(bytes, p + 22))?,
        };
        if setting.modulation != 0 || setting.start_position != 0 {
            warning(
                warnings,
                p,
                "GAX sample offset or modulation settings cannot yet be preserved in XM",
            );
        }
        sample_settings.push(setting);
    }
    let envelope_at = mapped(bytes, at + 12, 4, 4)?;
    let count = usize::from(bytes[envelope_at]);
    check(count > 0 && count <= 64)?;
    need(bytes.get(envelope_at..envelope_at + 4 + count * 8))?;
    let optional = |value: u8| -> ReadResult<Option<u8>> {
        if value == 255 {
            Ok(None)
        } else {
            check(usize::from(value) < count)?;
            Ok(Some(value))
        }
    };
    let sustain = optional(bytes[envelope_at + 1])?;
    let loop_start = optional(bytes[envelope_at + 2])?;
    let loop_end = optional(bytes[envelope_at + 3])?;
    check(loop_start.is_some() == loop_end.is_some() && loop_start <= loop_end)?;
    let mut points: Vec<GaxEnvelopePoint> = Vec::with_capacity(count);
    for i in 0..count {
        budget.charge()?;
        let p = envelope_at + 4 + i * 8;
        let point = GaxEnvelopePoint {
            tick: need(u16_at(bytes, p))?,
            interpolation: need(u16_at(bytes, p + 2))? as i16,
            volume: bytes[p + 4],
        };
        check(points.last().is_none_or(|last| last.tick < point.tick))?;
        points.push(point);
    }
    if count > 12 {
        warning(
            warnings,
            envelope_at,
            "GAX envelope exceeds XM's twelve-point limit",
        );
    }
    if points[0].tick != 0 {
        warning(warnings, envelope_at, "GAX envelope starts after tick zero");
    }
    for pair in points.windows(2) {
        let expected = (i32::from(pair[1].volume) - i32::from(pair[0].volume)) * 256
            / i32::from(pair[1].tick - pair[0].tick);
        if (i32::from(pair[1].interpolation) - expected).abs() > 1 {
            warning(
                warnings,
                envelope_at,
                "GAX envelope interpolation differs from a linear XM envelope",
            );
        }
    }
    let envelope = GaxEnvelope {
        source: RomSpan::new(envelope_at, 4 + count * 8),
        sustain,
        loop_start,
        loop_end,
        points,
    };
    Ok(GaxInstrument {
        index,
        blank: bytes[at] != 0,
        pointer: RomSpan::new(pointer, 4),
        descriptor: RomSpan::new(at, 24 + slots * 24),
        sample_indices,
        vibrato,
        row_speed: bytes[at + 16],
        rows_span: RomSpan::new(rows_at, row_count * 8),
        rows,
        sample_settings,
        envelope,
    })
}
