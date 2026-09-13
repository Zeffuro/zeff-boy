use std::collections::BTreeSet;

use crate::Budget;

use super::{GbGhxHardware, GbGhxSong, GbGhxTrack, ReadError, profiles::Driver, span, spans, word};

struct Reader<'a, 'b, 'c> {
    bytes: &'a [u8],
    driver: &'a Driver,
    budget: &'b mut Budget<'c>,
    mapped: Vec<(usize, usize)>,
    instruments: BTreeSet<(usize, u8)>,
}

impl<'a, 'b, 'c> Reader<'a, 'b, 'c> {
    fn read(&mut self, pointer: usize, len: usize) -> Result<&'a [u8], ReadError> {
        self.budget.charge()?;
        if pointer < 0x4000 || pointer + len > 0x8000 {
            return Err(ReadError::Invalid);
        }
        let at = self.driver.offset(pointer);
        self.mapped.push((at, at + len));
        self.bytes.get(at..at + len).ok_or(ReadError::Invalid)
    }

    fn pointer(&mut self, at: usize) -> Result<usize, ReadError> {
        Ok(usize::from(word(self.read(at, 2)?, 0)))
    }

    fn instrument(&mut self, at: usize, channel: u8) -> Result<(), ReadError> {
        if !self.instruments.insert((at, channel)) {
            return Ok(());
        }
        let header = self.read(at, 3)?;
        if channel == 2 && header[0] & 0xe0 != 0 {
            return Err(ReadError::Invalid);
        }
        let count = usize::from(header[0] & 63);
        let size = 3
            + usize::from(header[1] & 128 != 0 && channel != 3) * 2
            + usize::from(channel == 2) * 11;
        self.read(at, size)?;
        if channel == 2 && self.driver.profile.wave_table {
            self.wave_table(at + size - 11)?;
        }
        let steps = self.read(at + size, count * 3)?;
        for (offset, &value) in steps.iter().enumerate() {
            self.budget.charge()?;
            if offset % 3 != 0 && value & 0xc0 == 0x80 {
                let distance = usize::from(value & 63);
                if distance == 0 || distance * 3 > offset + 1 {
                    return Err(ReadError::Invalid);
                }
            }
        }
        Ok(())
    }

    fn wave_table(&mut self, at: usize) -> Result<(), ReadError> {
        let header = self.read(at, 11)?;
        let mut delta = header[0] as i8;
        let mut offset = word(header, 2);
        let low = word(header, 4);
        let high = word(header, 6);
        let base = usize::from(word(header, 9));
        let mut seen = BTreeSet::new();
        // Modulation reverses only on exact endpoints; missed endpoints must remain bounded.
        for _ in 0..32768 {
            if !seen.insert((offset, delta)) {
                return Ok(());
            }
            self.read(base + usize::from(offset), 16)?;
            offset = offset.wrapping_add_signed(i16::from(delta));
            if delta < 0 && offset == low || delta >= 0 && offset == high {
                delta = delta.wrapping_neg();
            }
        }
        Err(ReadError::Invalid)
    }

    fn pattern(
        &mut self,
        mut pointer: usize,
        rows: u8,
        channel: u8,
        instruments: usize,
    ) -> Result<u32, ReadError> {
        let mut notes = 0;
        for _ in 0..rows {
            let flags = self.read(pointer, 1)?[0];
            pointer += 1;
            notes += u32::from(flags & 63 != 0);
            if flags & 64 != 0 {
                let instrument = self.read(pointer, 1)?[0] & 63;
                pointer += 1;
                if instrument != 0 {
                    let instrument = self.pointer(instruments + usize::from(instrument - 1) * 2)?;
                    self.instrument(instrument, channel)?;
                }
            }
            if flags & 128 != 0 {
                let effect = self.read(pointer, 1)?[0];
                pointer += 1;
                if effect & 15 == 8 && effect >> 4 != 0 {
                    return Err(ReadError::Invalid);
                }
            }
        }
        Ok(notes)
    }
}

#[test]
fn wave_modulation_maps_both_directions_and_rejects_escaping_cycles() {
    use std::sync::atomic::AtomicBool;
    let mut bytes = super::tests::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    let driver = super::profiles::recognized(&bytes, &mut budget)
        .unwrap()
        .remove(0);
    bytes[0x8400..0x840b].copy_from_slice(&[2, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0x45]);
    for (delta, valid) in [(2, true), (0, true), (3, false), (128, false)] {
        bytes[0x8400] = delta;
        let mut reader = Reader {
            bytes: &bytes,
            driver: &driver,
            budget: &mut budget,
            mapped: Vec::new(),
            instruments: BTreeSet::new(),
        };
        assert_eq!(reader.wave_table(0x4400).is_ok(), valid);
        if delta == 2 {
            let mapped = spans(reader.mapped);
            assert!(
                mapped
                    .iter()
                    .any(|span| { span.effective_offset == 0x8500 && span.byte_len == 20 })
            );
        }
    }
}

pub(super) fn song(
    bytes: &[u8],
    driver: &Driver,
    module: u8,
    subsong: u8,
    budget: &mut Budget<'_>,
) -> Result<GbGhxSong, ReadError> {
    let mut reader = Reader {
        bytes,
        driver,
        budget,
        mapped: driver
            .code
            .iter()
            .map(|&(at, len)| {
                let start = driver.offset(0x4000 + at);
                (start, start + len)
            })
            .collect(),
        instruments: BTreeSet::new(),
    };
    for &constant in &driver.constants {
        reader.read(usize::from(constant), 512)?;
    }
    let mut header_at = 0;
    for index in 0..=module {
        header_at = reader.pointer(usize::from(driver.table) + usize::from(index) * 2)?;
        if reader.read(header_at, 3)? != b"GHX" {
            return Err(ReadError::Invalid);
        }
    }
    let header = reader.read(header_at, 12)?;
    if subsong >= header[3] || header[4] == 0 {
        return Err(ReadError::Invalid);
    }
    let rows = header[4];
    let patterns = usize::from(word(header, 6));
    let instruments = usize::from(word(header, 8));
    let setting = usize::from(word(header, 10)) + usize::from(subsong) * 6;
    let settings = reader.read(setting, 6)?;
    let mut notes = [0; 4];
    let stride = if driver.profile.direct_patterns {
        11
    } else {
        7
    };
    for phase in [0, 3] {
        let count = usize::from(settings[phase]) + 1;
        let orders = reader.read(usize::from(word(settings, phase + 1)), count * stride)?;
        for order in orders.chunks_exact(stride) {
            for channel in 0..4 {
                let pointer = if driver.profile.direct_patterns {
                    usize::from(word(order, channel * 3))
                } else {
                    reader.pointer(patterns + usize::from(order[channel * 2]) * 2)?
                };
                notes[channel] += reader.pattern(pointer, rows, channel as u8, instruments)?;
            }
        }
    }
    if notes.iter().all(|&count| count == 0) {
        return Err(ReadError::Invalid);
    }
    Ok(GbGhxSong {
        profile: driver.profile.name,
        index: u16::from(module) * 256 + u16::from(subsong),
        module,
        subsong,
        title: format!("Song {}", u16::from(module) * 256 + u16::from(subsong)),
        bank: driver.bank,
        hardware: GbGhxHardware::CgbDouble,
        table_entry: span(driver.offset(setting), 6),
        tracks: notes
            .into_iter()
            .enumerate()
            .map(|(channel, note_count)| GbGhxTrack {
                number: channel as u8 + 1,
                note_count,
            })
            .collect(),
        mapped_spans: spans(reader.mapped),
        warnings: Vec::new(),
    })
}
