use crate::hardware::vdp::FieldPosition;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_emu_common::time::MasterTicks;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    field: u64,
    row: u16,
    horizontal: Option<[u16; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ScrollFetch {
    held: Option<[u16; 2]>,
    rows: [Option<Row>; 2],
}

impl ScrollFetch {
    pub(super) fn fetch(&mut self, field: u64, row: u16, enabled: bool, pair: Option<[u16; 2]>) {
        self.hold(enabled, pair);
        self.rows[usize::from(row & 1)] = Some(Row {
            field,
            row,
            horizontal: self.held,
        });
    }

    pub(super) fn hold(&mut self, enabled: bool, pair: Option<[u16; 2]>) {
        // A disabled fetch retains the previous pair.
        if enabled {
            self.held = pair;
        }
    }

    pub(super) fn horizontal(&self, field: u64, row: u16) -> Option<[u16; 2]> {
        self.rows[usize::from(row & 1)]
            .filter(|entry| entry.field == field && entry.row == row)
            .and_then(|entry| entry.horizontal)
    }

    pub(super) fn clear_rows(&mut self) {
        self.rows = [None; 2];
    }

    pub(super) fn validate(
        &self,
        position: FieldPosition,
        height: usize,
        now: MasterTicks,
        fetch_phase: Option<u64>,
        next_field: bool,
    ) -> Result<()> {
        let pair_valid =
            |pair: Option<[u16; 2]>| pair.is_none_or(|pair| pair.into_iter().all(|v| v <= 0x3ff));
        ensure!(pair_valid(self.held), "invalid MD held horizontal scroll");
        for (slot, entry) in self.rows.into_iter().enumerate() {
            if let Some(entry) = entry {
                ensure!(
                    usize::from(entry.row) < height
                        && usize::from(entry.row & 1) == slot
                        && pair_valid(entry.horizontal),
                    "invalid MD row horizontal scroll"
                );
                ensure!(
                    entry.field.saturating_add(1) >= position.id,
                    "stale MD scroll fetch field"
                );
                ensure!(
                    entry.field <= position.id
                        || (position.id.checked_add(1) == Some(entry.field)
                            && next_field
                            && entry.row == 0),
                    "MD scroll fetch is ahead of its calendar"
                );
                if entry.field == position.id {
                    ensure!(
                        entry.row <= position.line.saturating_add(1),
                        "MD scroll row is ahead of its calendar"
                    );
                }
                if entry.field > position.id
                    || (entry.field == position.id && entry.row == position.line.saturating_add(1))
                {
                    ensure!(
                        fetch_phase
                            .is_some_and(|phase| now.get() - position.line_start.get() >= phase),
                        "MD scroll row precedes its fetch"
                    );
                }
            }
        }
        Ok(())
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        write_pair(writer, self.held);
        for entry in self.rows {
            writer.write_bool(entry.is_some());
            if let Some(entry) = entry {
                writer.write_u64(entry.field);
                writer.write_u16(entry.row);
                write_pair(writer, entry.horizontal);
            }
        }
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let held = read_pair(reader)?;
        let mut rows = [None; 2];
        for (slot, entry) in rows.iter_mut().enumerate() {
            if reader.read_bool()? {
                let row = Row {
                    field: reader.read_u64()?,
                    row: reader.read_u16()?,
                    horizontal: read_pair(reader)?,
                };
                ensure!(
                    row.row < 240 && usize::from(row.row & 1) == slot,
                    "invalid MD scroll row"
                );
                *entry = Some(row);
            }
        }
        Ok(Self { held, rows })
    }
}

fn write_pair(writer: &mut StateWriter, pair: Option<[u16; 2]>) {
    writer.write_bool(pair.is_some());
    if let Some(pair) = pair {
        for value in pair {
            writer.write_u16(value);
        }
    }
}

fn read_pair(reader: &mut StateReader<'_>) -> Result<Option<[u16; 2]>> {
    Ok(if reader.read_bool()? {
        let pair = [reader.read_u16()?, reader.read_u16()?];
        ensure!(
            pair.into_iter().all(|value| value <= 0x3ff),
            "invalid MD horizontal scroll"
        );
        Some(pair)
    } else {
        None
    })
}
